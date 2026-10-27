//! VM とネイティブで共有するランタイムの核 (docs/spec/object-model.md)。語、ヘッダ、
//! 確保器、RC、`share`、reuse の部品、検査モードを持つ。まだどの crate からも使わない。

// Verus の `while let` では条件で取り出す前の値を ghost の値に取れないので、`loop` と `match` で書くループがある
#![allow(clippy::while_let_loop)]

// Verus は crate の根に vstd の prelude を求める
#[allow(unused_imports)]
use vstd::prelude::*;

mod alloc;
mod cell;
mod control;
mod error;
pub mod externs;
mod foreign;
mod frozen;
mod heap;
mod host;
mod meta;
mod mode;
mod rc;
mod text;
#[cfg(test)]
mod trust;
mod word;

pub use control::{Control, Limits, Stop};
pub use error::HeapError;
pub use externs::{
    ArithFault, Fault, Form, Handle, IndexFault, IoOp, IoReason, Open, Raised, ReadAmount,
    StdStream, Utf8Source, Write,
};
pub use frozen::Frozen;
pub use heap::{Ctx, CtxStats, Heap, MaskItem, Stats, Token};
pub use host::{
    Buffering, Host, HostRef, Opened, Process, Spawn, Spawned, StdHost, StderrMode, StdinSlot,
    Stdio, Stream, WriteMode, exit_code,
};
pub use meta::{Kind, Meta};
pub use mode::{Checked, Fast, Mode};
pub use word::Word;

/// `Heap` と `Ctx` が `Sync` でないことをコンパイルの時点で確かめる。`Sync` なら下の2つの実装のどちらを選ぶかが
/// 決まらず、コンパイルが止まる (static_assertions の `assert_not_impl_any!` と同じ方法)。
const _: fn() = || {
    trait AmbiguousIfSync<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> AmbiguousIfSync<()> for T {}
    impl<T: ?Sized + Sync> AmbiguousIfSync<u8> for T {}
    let _ = <Heap<Fast> as AmbiguousIfSync<_>>::some_item;
    let _ = <Heap<Checked> as AmbiguousIfSync<_>>::some_item;
    let _ = <Ctx<Fast> as AmbiguousIfSync<_>>::some_item;
    let _ = <Ctx<Checked> as AmbiguousIfSync<_>>::some_item;
};

// `Control` はセッションのどのスレッドからも同じものを見る
const _: fn() = || {
    fn send_and_sync<T: Send + Sync>() {}
    send_and_sync::<Control>();
};

#[cfg(test)]
mod tests {
    use super::*;

    fn send_and_sync<T: Send + Sync>() {}

    fn send<T: Send>() {}

    #[test]
    fn raw_bits_round_trip_through_a_word() {
        for n in [0i64, 1, 2, -1, i64::MIN, i64::MAX] {
            assert_eq!(Word::from_bits(n as usize).bits() as i64, n);
        }
    }

    #[test]
    fn words_cross_threads_and_heaps_move_to_one() {
        send_and_sync::<Word>();
        send_and_sync::<Frozen<Fast>>();
        send_and_sync::<Frozen<Checked>>();
        send::<Heap<Fast>>();
        send::<Heap<Checked>>();
        send::<Ctx<Fast>>();
        send::<Ctx<Checked>>();
    }
}
