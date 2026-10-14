//! 実行の制御 (docs/spec/object-model.md の「実行の制御」)。

use std::sync::Arc;

use super::host::host;
use crate::{Control, Ctx, Heap, Limits, Mode, Stop};

const PAGE_BYTES: usize = 0x1_0000;

fn heaps_of_one_session_add_up_their_pages<M: Mode>() {
    let control = Arc::new(Control::new(Limits::default()));
    let before = control.bytes();
    let a = Heap::<M>::with_control(control.clone());
    let b = Heap::<M>::with_control(control.clone());
    assert_eq!(
        control.bytes(),
        before + a.bytes_in_use_pages() + b.bytes_in_use_pages()
    );
    drop(a);
    drop(b);
    assert_eq!(control.bytes(), before);
}

fn memory_limit_sets_the_stop_bit<M: Mode>() {
    let control = Arc::new(Control::new(Limits {
        max_bytes: 3 * PAGE_BYTES,
        max_depth: 100,
        ..Limits::default()
    }));
    let mut heap = Heap::<M>::with_control(control.clone());
    let ctx = Ctx::<M>::new(control.clone(), host(&Arc::default()));
    assert_eq!(control.stop_bits(), 0);
    assert_eq!(ctx.poll(0), Ok(()));
    // 大きい物体を1つずつ取り、上限を超えるまでページを足す。1つで1ページを超えるので、数回で上限に届く
    let mut held = Vec::new();
    while control.stop_bits() & Control::MEMORY == 0 {
        assert!(held.len() < 4);
        held.push(heap.alloc_data(0, 0, PAGE_BYTES / 8, false));
    }
    assert!(control.bytes() > 3 * PAGE_BYTES);
    // 割り込みを確保量より先に、確保量を深さより先に報告する
    assert_eq!(ctx.poll(0), Err(Stop::MemoryLimit));
    assert_eq!(ctx.poll(101), Err(Stop::MemoryLimit));
    control.interrupt();
    assert_eq!(ctx.poll(101), Err(Stop::Interrupted));
    for w in held {
        unsafe { heap.decref(w) }.unwrap();
    }
    drop(heap);
    drop(ctx);
    assert_eq!(control.bytes(), 0);
    // 下回っても、clear するまでビットは立ったままである
    assert_ne!(control.stop_bits() & Control::MEMORY, 0);
    control.clear();
    assert_eq!(control.stop_bits(), 0);
}

/// 確保量が上限にちょうど等しいときは、ビットを立てない。
fn reaching_the_memory_limit_exactly_sets_no_bit<M: Mode>() {
    let control = Arc::new(Control::new(Limits {
        max_bytes: PAGE_BYTES,
        ..Limits::default()
    }));
    let heap = Heap::<M>::with_control(control.clone());
    assert_eq!(control.bytes(), PAGE_BYTES);
    assert_eq!(control.stop_bits(), 0);
    let _more = Heap::<M>::with_control(control.clone());
    assert_ne!(control.stop_bits() & Control::MEMORY, 0);
    drop(heap);
}

// `Control` はヒープのモードによらないので、1つのテストにする
#[test]
fn interrupt_and_clear() {
    let control = Control::new(Limits::default());
    control.interrupt();
    assert_eq!(control.stop_bits(), Control::INTERRUPT);
    control.clear();
    assert_eq!(control.stop_bits(), 0);
}

fn frozen_chunks_stay_counted_until_dropped<M: Mode>() {
    let control = Arc::new(Control::new(Limits::default()));
    let mut b = Heap::<M>::with_control(control.clone());
    let _ = b.alloc_string_immortal("constant");
    let pages = b.bytes_in_use_pages();
    let region = b.freeze().unwrap();
    // 作り手のヒープは消えたが、塊が引き継いだページは塊が生きている間は数に入る
    assert_eq!(control.bytes(), pages);
    drop(region);
    assert_eq!(control.bytes(), 0);
}

both_modes!(
    heaps_of_one_session_add_up_their_pages,
    memory_limit_sets_the_stop_bit,
    reaching_the_memory_limit_exactly_sets_no_bit,
    frozen_chunks_stay_counted_until_dropped,
);
