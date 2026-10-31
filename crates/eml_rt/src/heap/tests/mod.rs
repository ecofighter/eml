//! ヒープの単体テスト (docs/spec/object-model.md の「核の操作」)。モードを型の引数に
//! 取る関数を `both_modes!` で2つのモードのテストにし、同じ列を速いモードと検査モードで流す。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{Checked, Ctx, Heap, HeapError, Mode, Word};

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

mod arrays;
mod basics;
mod bytes;
mod checked;
mod control;
mod ctx;
mod externs;
mod frozen;
mod host;
mod raw;
mod recover;
mod remote;
mod reuse;
mod roots;
mod script;
mod share;
mod string;
mod table;
mod transfer;
mod trusted;
mod workers;

/// 生存の列挙の検査と、ID ごとの数の数え直し。VM の検査モードが run の後に確かめるものと同じである。
pub(super) fn check_all<L: Live<M = Checked>>(live: &mut L) -> Result<(), HeapError> {
    let heap = live.heap_for_walk();
    heap.check_invariants()?;
    heap.control().check_owned(&[heap])
}

/// 解放の連鎖と `share` が再帰しないことを確かめる長さ。
pub(super) const LONG: usize = 100_000;

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

/// ダミーのほかに生きている箱の数を数えられるもの。生存の列挙は先に取り込むので、ヒープを書き換える。
pub(super) trait Live {
    fn heap_for_walk(&mut self) -> &mut Heap<Self::M>;
    type M: Mode;
}

impl<M: Mode> Live for Heap<M> {
    type M = M;
    fn heap_for_walk(&mut self) -> &mut Heap<M> {
        self
    }
}

impl<M: Mode> Live for Ctx<M> {
    type M = M;
    fn heap_for_walk(&mut self) -> &mut Heap<M> {
        // 生存の列挙は、取り込みのほかにヒープを変えない
        unsafe { self.heap_mut() }
    }
}

/// ダミーのほかに生きている箱の数。
pub(super) fn mortal<L: Live>(live: &mut L) -> usize {
    live.heap_for_walk().live_objects().len() - 1
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
