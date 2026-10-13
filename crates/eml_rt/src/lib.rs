//! VM とネイティブで共有するランタイムの核 (docs/spec/object-model.md)。語、ヘッダ、
//! 確保器、RC、`share`、reuse の部品、検査モードを持つ。まだどの crate からも使わない。

// Verus の `while let` では条件で取り出す前の値を ghost の値に取れないので、`loop` と `match` で書くループがある
#![allow(clippy::while_let_loop)]

// Verus は crate の根に vstd の prelude を求める
#[allow(unused_imports)]
use vstd::prelude::*;

mod alloc;
mod cell;
mod error;
mod foreign;
mod heap;
mod meta;
mod mode;
mod rc;
#[cfg(test)]
mod trust;
mod word;

pub use error::HeapError;
pub use heap::{Heap, Stats, Token};
pub use meta::{Kind, Meta};
pub use mode::{Checked, Fast, Mode};
pub use word::Word;

/// `Heap` が `Sync` でないことをコンパイルの時点で確かめる。`Sync` なら下の2つの実装のどちらを選ぶかが決まらず、
/// コンパイルが止まる (static_assertions の `assert_not_impl_any!` と同じ方法)。
const _: fn() = || {
    trait AmbiguousIfSync<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> AmbiguousIfSync<()> for T {}
    impl<T: ?Sized + Sync> AmbiguousIfSync<u8> for T {}
    let _ = <Heap<Fast> as AmbiguousIfSync<_>>::some_item;
    let _ = <Heap<Checked> as AmbiguousIfSync<_>>::some_item;
};

#[cfg(test)]
mod tests {
    use super::*;

    fn send_and_sync<T: Send + Sync>() {}

    fn send<T: Send>() {}

    #[test]
    fn words_cross_threads_and_heaps_move_to_one() {
        send_and_sync::<Word>();
        send::<Heap<Fast>>();
        send::<Heap<Checked>>();
    }
}
