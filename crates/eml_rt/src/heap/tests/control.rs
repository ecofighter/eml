//! 実行の制御 (docs/spec/object-model.md の「確保と解放」)。

use std::sync::Arc;

use crate::{Control, Heap, Limits, Mode};

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
        ..Limits::default()
    }));
    let mut heap = Heap::<M>::with_control(control.clone());
    assert_eq!(control.stop_bits(), 0);
    // 大きい物体を1つずつ取り、上限を超えるまでページを足す
    let mut held = Vec::new();
    while control.stop_bits() & Control::MEMORY == 0 {
        held.push(heap.alloc_data(0, 0, PAGE_BYTES / 8, false));
    }
    assert!(control.bytes() > 3 * PAGE_BYTES);
    for w in held {
        unsafe { heap.decref(w) }.unwrap();
    }
    drop(heap);
    // 下回っても、clear するまでビットは立ったままである
    assert_ne!(control.stop_bits() & Control::MEMORY, 0);
    control.clear();
    assert_eq!(control.stop_bits(), 0);
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
    let region = b.freeze().unwrap();
    // 作り手のヒープは消えたが、塊のページは塊が生きている間は数に入る
    assert!(control.bytes() > 0);
    drop(region);
    assert_eq!(control.bytes(), 0);
}

both_modes!(
    heaps_of_one_session_add_up_their_pages,
    memory_limit_sets_the_stop_bit,
    frozen_chunks_stay_counted_until_dropped,
);
