//! 実行の制御 (docs/spec/object-model.md の「実行の制御」)。

use std::sync::Arc;

use super::host::host;
use crate::control::stop_reason;
use crate::{Control, Ctx, Fast, Heap, Limits, Mode, Stop};

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

// `PREEMPT` は worker の語だけに立ち、host の読み手が見る `stop` には立たない
#[test]
fn preempt_reaches_only_the_worker_word() {
    let control = Arc::new(Control::new(Limits::default()));
    let ctx = Ctx::<Fast>::new(control.clone(), host(&Arc::default()));
    control.preempt();
    assert_eq!(control.worker_bits(), Control::PREEMPT);
    assert_eq!(control.stop_bits(), 0);
    assert_eq!(ctx.poll(0), Err(Stop::Preempt));
    control.take_preempt();
    assert_eq!(control.worker_bits(), 0);
    assert_eq!(ctx.poll(0), Ok(()));
    // 割り込みは両方の語に立つ。`take_preempt` は割り込みを下ろさない
    control.interrupt();
    control.preempt();
    control.take_preempt();
    assert_eq!(control.worker_bits(), Control::INTERRUPT);
    assert_eq!(control.stop_bits(), Control::INTERRUPT);
}

#[test]
fn the_memory_limit_reaches_the_worker_word() {
    let control = Control::new(Limits {
        max_bytes: 10,
        ..Limits::default()
    });
    control.charge(10);
    assert_eq!(control.worker_bits(), 0);
    assert_eq!(control.stop_bits(), 0);
    control.charge(1);
    assert_eq!(control.worker_bits(), Control::MEMORY);
    assert_eq!(control.stop_bits(), Control::MEMORY);
}

#[test]
fn begin_run_reopens_the_end_and_lowers_preempt() {
    let control = Control::new(Limits::default());
    assert!(control.exit(7));
    control.interrupt();
    control.preempt();
    control.begin_run();
    assert_eq!(control.exit_code(), None);
    // 止めるビットは下ろさない
    assert_eq!(control.worker_bits(), Control::INTERRUPT);
    assert_eq!(control.stop_bits(), Control::INTERRUPT);
    assert!(control.exit(8));
    assert_eq!(control.exit_code(), Some(8));
}

#[test]
fn clear_lowers_every_bit_and_the_exit() {
    let control = Control::new(Limits::default());
    control.interrupt();
    control.preempt();
    assert!(control.exit(3));
    control.clear();
    assert_eq!(control.stop_bits(), 0);
    assert_eq!(control.worker_bits(), 0);
    assert_eq!(control.exit_code(), None);
}

// `NOTICE` も worker の語だけに立つ。`poll` は止めずに続ける知らせとして `Notice` を返す
#[test]
fn notice_reaches_only_the_worker_word() {
    let control = Arc::new(Control::new(Limits::default()));
    let ctx = Ctx::<Fast>::new(control.clone(), host(&Arc::default()));
    control.notice();
    assert_eq!(control.worker_bits(), Control::NOTICE);
    assert_eq!(control.stop_bits(), 0);
    assert_eq!(ctx.poll(0), Err(Stop::Notice));
    control.take_notice();
    assert_eq!(control.worker_bits(), 0);
    assert_eq!(ctx.poll(0), Ok(()));
}

#[test]
fn stop_reason_puts_notice_between_the_stops_and_preempt() {
    let notice = Control::NOTICE | Control::PREEMPT;
    assert_eq!(stop_reason(notice), Stop::Notice);
    assert_eq!(stop_reason(Control::NOTICE), Stop::Notice);
    assert_eq!(stop_reason(notice | Control::MEMORY), Stop::MemoryLimit);
    assert_eq!(stop_reason(notice | Control::TERMINATE), Stop::Terminated);
    assert_eq!(
        stop_reason(Control::TERMINATE | Control::MEMORY),
        Stop::Terminated
    );
    assert_eq!(
        stop_reason(Control::TERMINATE | Control::INTERRUPT),
        Stop::Interrupted
    );
}

#[test]
fn a_claimed_signal_is_returned_once_until_it_is_settled() {
    let control = Control::new(Limits::default());
    assert_eq!(control.claim_signals(), 0);
    control.defer(Control::INTERRUPT);
    assert_eq!(control.pending_signals(), Control::INTERRUPT);
    assert_eq!(control.claim_signals(), Control::INTERRUPT);
    assert_eq!(control.claim_signals(), 0);
    // 中継済みでも、届けるまでは保留のままである
    assert_eq!(control.pending_signals(), Control::INTERRUPT);
    // 中継した後に来た別の種類だけを返す
    control.defer(Control::TERMINATE);
    control.defer(Control::INTERRUPT);
    assert_eq!(control.claim_signals(), Control::TERMINATE);
    assert_eq!(
        control.pending_signals(),
        Control::INTERRUPT | Control::TERMINATE
    );
    // 届けた種類は保留も中継済みも下りるので、次の保留はまた1回返る
    control.settle(Control::INTERRUPT);
    assert_eq!(control.pending_signals(), Control::TERMINATE);
    assert_eq!(control.claim_signals(), 0);
    control.defer(Control::INTERRUPT);
    assert_eq!(control.claim_signals(), Control::INTERRUPT);
    // シグナルの語は止める語に出ない
    assert_eq!(control.stop_bits(), 0);
    assert_eq!(control.worker_bits(), 0);
}

#[test]
fn take_pending_empties_the_signal_word() {
    let control = Control::new(Limits::default());
    control.defer(Control::TERMINATE);
    assert_eq!(control.claim_signals(), Control::TERMINATE);
    control.defer(Control::INTERRUPT);
    assert_eq!(
        control.take_pending(),
        Control::INTERRUPT | Control::TERMINATE
    );
    assert_eq!(control.pending_signals(), 0);
    assert_eq!(control.take_pending(), 0);
    // 中継済みの印も消えたので、次の保留はまた返る
    control.defer(Control::TERMINATE);
    assert_eq!(control.claim_signals(), Control::TERMINATE);
}

#[test]
fn watched_keeps_the_last_bits() {
    let control = Control::new(Limits::default());
    assert_eq!(control.watched(), 0);
    control.set_watched(Control::INTERRUPT | Control::TERMINATE);
    assert_eq!(control.watched(), Control::INTERRUPT | Control::TERMINATE);
    control.set_watched(Control::TERMINATE);
    assert_eq!(control.watched(), Control::TERMINATE);
}

#[test]
fn begin_run_lowers_notice_but_keeps_the_signals() {
    let control = Control::new(Limits::default());
    control.terminate();
    control.notice();
    control.preempt();
    control.defer(Control::INTERRUPT);
    control.set_watched(Control::INTERRUPT);
    control.begin_run();
    assert_eq!(control.worker_bits(), Control::TERMINATE);
    assert_eq!(control.stop_bits(), Control::TERMINATE);
    assert_eq!(control.pending_signals(), Control::INTERRUPT);
    assert_eq!(control.watched(), Control::INTERRUPT);
}

#[test]
fn clear_lowers_the_signals_and_the_watched_bits() {
    let control = Control::new(Limits::default());
    control.terminate();
    control.notice();
    control.defer(Control::TERMINATE);
    assert_eq!(control.claim_signals(), Control::TERMINATE);
    control.set_watched(Control::TERMINATE);
    control.clear();
    assert_eq!(control.stop_bits(), 0);
    assert_eq!(control.worker_bits(), 0);
    assert_eq!(control.pending_signals(), 0);
    assert_eq!(control.watched(), 0);
    control.defer(Control::TERMINATE);
    assert_eq!(control.claim_signals(), Control::TERMINATE);
}

#[test]
fn stop_reason_puts_preempt_after_the_stops() {
    let both = Control::PREEMPT | Control::INTERRUPT;
    assert_eq!(stop_reason(Control::PREEMPT), Stop::Preempt);
    assert_eq!(stop_reason(both), Stop::Interrupted);
    assert_eq!(
        stop_reason(Control::PREEMPT | Control::MEMORY),
        Stop::MemoryLimit
    );
    assert_eq!(stop_reason(0), Stop::DepthLimit);
}

#[test]
fn the_first_exit_is_kept() {
    let control = Control::new(Limits::default());
    assert_eq!(control.exit_code(), None);
    assert!(control.exit(300));
    assert!(!control.exit(1));
    assert_eq!(control.exit_code(), Some(300));
    // 終わりは止める語に出ない
    assert_eq!(control.stop_bits(), 0);
    assert_eq!(control.worker_bits(), 0);
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
