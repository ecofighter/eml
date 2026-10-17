//! 実行文脈と基本操作 (docs/spec/object-model.md の「実行文脈 `Ctx`」)。

use std::sync::Arc;

use super::host::{MemoryHost, host};
use crate::{Control, Ctx, HeapError, Limits, MaskItem, Mode, Stop, Word};

pub(super) const UNIT: Word = Word::imm(0);

pub(super) fn ctx<M: Mode>() -> Ctx<M> {
    ctx_with(&Arc::new(MemoryHost::default()))
}

pub(super) fn ctx_with<M: Mode>(memory: &Arc<MemoryHost>) -> Ctx<M> {
    Ctx::new(Arc::new(Control::new(Limits::default())), host(memory))
}

/// エフェクト 0、操作1つの evidence を作る。外側の evv は空である。
pub(super) fn ctx_evidence<M: Mode>(ctx: &mut Ctx<M>) -> Word {
    unsafe { ctx.ev_new(0, 1, UNIT, UNIT) }.unwrap()
}

/// 今の evv の先頭に、エフェクト `effect` の新しい evidence を足し、その evidence を返す (呼ぶ側が1つ参照を持つ)。
pub(super) fn push<M: Mode>(ctx: &mut Ctx<M>, effect: u16, n_ops: usize) -> Word {
    unsafe {
        let outer = ctx.evv_get().unwrap();
        let ev = ctx.ev_new(effect, n_ops, UNIT, outer).unwrap();
        ctx.heap_mut().dup(ev).unwrap();
        let list = ctx.evv_get().unwrap();
        let list = ctx.evv_push(list, ev).unwrap();
        ctx.evv_set(list).unwrap();
        ev
    }
}

/// エフェクト `effect` の、先頭の `keep` 個を残して続く `skip` 個を除く mask の項目。
pub(super) fn item(effect: u16, keep: u32, skip: u32) -> MaskItem {
    MaskItem { effect, keep, skip }
}

/// 今の evv を空にする。
pub(super) fn clear<M: Mode>(ctx: &mut Ctx<M>) {
    unsafe { ctx.evv_set(UNIT) }.unwrap();
}

fn a_new_ctx_is_finished<M: Mode>() {
    let ctx = ctx::<M>();
    assert!(ctx.check_finished());
    assert_eq!(ctx.yielding(), 0);
    assert_eq!(ctx.poll(0), Ok(()));
}

fn poll_reports_each_stop<M: Mode>() {
    let control = Arc::new(Control::new(Limits {
        max_depth: 100,
        ..Limits::default()
    }));
    let ctx = Ctx::<M>::new(control.clone(), host(&Arc::default()));
    assert_eq!(ctx.poll(100), Ok(()));
    assert_eq!(ctx.poll(101), Err(Stop::DepthLimit));
    control.interrupt();
    assert_eq!(ctx.poll(0), Err(Stop::Interrupted));
    control.clear();
    assert_eq!(ctx.poll(0), Ok(()));
}

/// yield して、継続を2回組み立て直し、終える。最初の κ は関数の値 (`Word::imm(1)`) で、関数 0 の値は `()` と同じ語に
/// なるので使わない。
fn yields_and_continuation_frames_are_counted<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = ctx_evidence(&mut ctx);
    unsafe {
        ctx.yield_to(ev, UNIT, Word::imm(1)).unwrap();
        assert_eq!(ctx.yielding(), 1);
        assert_eq!(ctx.yield_status(ev), 1);
        for _ in 0..2 {
            let k = ctx.yield_cont_take().unwrap();
            ctx.yield_cont_set(k).unwrap();
        }
        let k = ctx.yield_cont_take().unwrap();
        ctx.heap_mut().decref(k).unwrap();
        let clause = ctx.yield_finish().unwrap();
        assert_eq!(clause, UNIT);
    }
    assert!(ctx.check_finished());
    if M::CHECKED {
        let stats = ctx.stats();
        assert_eq!((stats.yields, stats.yield_frames), (1, 2));
    }
}

fn yield_status_tells_targets_apart<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = ctx_evidence(&mut ctx);
    let other = ctx_evidence(&mut ctx);
    assert_eq!(ctx.yield_status(ev), 0);
    unsafe { ctx.yield_abort(ev, UNIT, UNIT) }.unwrap();
    assert_eq!(ctx.yield_status(ev), 2);
    assert_eq!(ctx.yield_status(other), 3);
    unsafe {
        ctx.yield_finish().unwrap();
        ctx.heap_mut().decref(other).unwrap();
    }
    assert!(ctx.check_finished());
}

fn misused_yields_are_reported_in_checked_mode<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        assert_eq!(ctx.yield_finish().unwrap_err(), HeapError::YieldMisuse);
        assert_eq!(ctx.yield_cont_take().unwrap_err(), HeapError::YieldMisuse);
        let ev = ctx_evidence(&mut ctx);
        ctx.yield_to(ev, UNIT, Word::imm(1)).unwrap();
        // κ を持ったまま、もう1つの κ は入れられない
        assert_eq!(
            ctx.yield_cont_set(Word::imm(2)).unwrap_err(),
            HeapError::YieldMisuse
        );
        // yield 中にもう1つの yield は始められない
        let ev2 = ctx_evidence(&mut ctx);
        assert_eq!(
            ctx.yield_to(ev2, UNIT, UNIT).unwrap_err(),
            HeapError::YieldMisuse
        );
        // κ を持ったまま終えられない
        assert_eq!(ctx.yield_finish().unwrap_err(), HeapError::YieldMisuse);
    }
}

fn evv_get_and_set_swap_the_list<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let empty = ctx.evv_get().unwrap();
        assert_eq!(empty, UNIT);
        ctx.evv_set(UNIT).unwrap();
    }
    assert!(ctx.check_finished());
}

fn dropping_a_ctx_mid_yield<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = ctx_evidence(&mut ctx);
    unsafe {
        let k = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.yield_to(ev, UNIT, k).unwrap();
    }
    assert!(!ctx.check_finished());
    drop(ctx);
}

fn lookup_finds_the_innermost_evidence<M: Mode>() {
    let mut ctx = ctx::<M>();
    let e0 = push(&mut ctx, 0, 1);
    let e1 = push(&mut ctx, 1, 1);
    let e0_inner = push(&mut ctx, 0, 1);
    unsafe {
        let found = ctx.ev_lookup(0).unwrap();
        assert_eq!(found, e0_inner);
        ctx.heap_mut().decref(found).unwrap();
        let found = ctx.ev_lookup(1).unwrap();
        assert_eq!(found, e1);
        ctx.heap_mut().decref(found).unwrap();
        for e in [e0, e1, e0_inner] {
            ctx.heap_mut().decref(e).unwrap();
        }
    }
    clear(&mut ctx);
    assert!(ctx.check_finished());
}

fn find_returns_the_innermost_evidence_or_none<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        assert_eq!(ctx.ev_find(0).unwrap(), None);
    }
    let e0 = push(&mut ctx, 0, 1);
    let e1 = push(&mut ctx, 1, 1);
    let e0_inner = push(&mut ctx, 0, 1);
    unsafe {
        let found = ctx.ev_find(0).unwrap().unwrap();
        assert_eq!(found, e0_inner);
        ctx.heap_mut().decref(found).unwrap();
        let found = ctx.ev_find(1).unwrap().unwrap();
        assert_eq!(found, e1);
        ctx.heap_mut().decref(found).unwrap();
        assert_eq!(ctx.ev_find(2).unwrap(), None);
        for e in [e0, e1, e0_inner] {
            ctx.heap_mut().decref(e).unwrap();
        }
    }
    clear(&mut ctx);
    assert!(ctx.check_finished());
}

fn exit_abort_targets_the_dummy<M: Mode>() {
    let mut ctx = ctx::<M>();
    let dummy = ctx.heap().dummy();
    unsafe {
        ctx.exit_abort().unwrap();
        assert_eq!(ctx.yield_status(dummy), 2);
        assert_eq!(ctx.yield_cont_take().unwrap(), Word::UNIT);
        assert_eq!(ctx.yield_finish().unwrap(), Word::UNIT);
    }
    assert!(ctx.check_finished());
}

fn clauses_states_and_outer_lists<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 0, 2);
    unsafe {
        let clause = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.ev_set_clause(ev, 1, clause).unwrap();
        let got = ctx.ev_clause(ev, 1).unwrap();
        assert_eq!(got, clause);
        ctx.heap_mut().decref(got).unwrap();
        assert_eq!(ctx.ev_clause(ev, 0).unwrap(), UNIT);
        // 状態の出し入れ
        let s = ctx.heap_mut().alloc_string("state");
        ctx.ev_set_state(ev, s).unwrap();
        let taken = ctx.ev_take_state(ev).unwrap();
        assert_eq!(taken, s);
        assert_eq!(ctx.ev_take_state(ev).unwrap(), UNIT);
        ctx.heap_mut().decref(taken).unwrap();
        // 外側の evv は、作ったときの evv (空) である
        assert_eq!(ctx.ev_evv(ev).unwrap(), UNIT);
        // renew は同じ入口の表を指し、外側を今の evv にする
        let renewed = ctx.ev_renew(ev, UNIT).unwrap();
        let got = ctx.ev_clause(renewed, 1).unwrap();
        assert_eq!(got, clause);
        ctx.heap_mut().decref(got).unwrap();
        let outer = ctx.ev_evv(renewed).unwrap();
        let now = ctx.evv_get().unwrap();
        assert_eq!(outer, now);
        for w in [outer, now, renewed, ev] {
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    clear(&mut ctx);
    assert!(ctx.check_finished());
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// 操作が 255 個以上のエフェクトの入口の表は、先頭に数の語を持つ (docs/spec/object-model.md の「物体の形」)。
fn entry_tables_with_a_count_word<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 0, 300);
    unsafe {
        let clause = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.ev_set_clause(ev, 299, clause).unwrap();
        assert_eq!(ctx.ev_clause(ev, 0).unwrap(), UNIT);
        let got = ctx.ev_clause(ev, 299).unwrap();
        assert_eq!(got, clause);
        ctx.heap_mut().decref(got).unwrap();
        ctx.heap_mut().decref(ev).unwrap();
    }
    clear(&mut ctx);
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// 状態の欄を通る輪 (docs/spec/lir.md の「evidence」): 状態が ev を指す物体を持っていても、ev を呼ぶ側が
/// 持っている限り、元の状態を手放しても ev は生き残る。
fn replacing_a_state_that_reaches_its_evidence<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 0, 1);
    unsafe {
        let holder = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.heap_mut().dup(ev).unwrap();
        ctx.heap_mut().set_field(holder, 0, ev).unwrap();
        ctx.ev_set_state(ev, holder).unwrap();
        ctx.ev_set_state(ev, UNIT).unwrap();
        // ev はまだ読める
        assert_eq!(ctx.ev_take_state(ev).unwrap(), UNIT);
        ctx.heap_mut().decref(ev).unwrap();
    }
    clear(&mut ctx);
    assert_eq!(super::mortal(ctx.heap()), 0);
}

fn bad_evidence_is_reported_in_checked_mode<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        // evidence でない物体 (tag 0 のクロージャ)
        let closure = ctx.heap_mut().alloc_data(0, 3, 0, false);
        assert_eq!(
            ctx.ev_clause(closure, 0).unwrap_err(),
            HeapError::BadEvidence
        );
        assert_eq!(
            ctx.ev_take_state(closure).unwrap_err(),
            HeapError::BadEvidence
        );
        // 範囲の外の操作
        let ev = push(&mut ctx, 0, 1);
        assert_eq!(ctx.ev_clause(ev, 1).unwrap_err(), HeapError::BadEvidence);
        // 入口の欄に2回入れる
        let c = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.ev_set_clause(ev, 0, c).unwrap();
        let c2 = ctx.heap_mut().alloc_data(0, 1, 0, false);
        assert_eq!(
            ctx.ev_set_clause(ev, 0, c2).unwrap_err(),
            HeapError::BadEvidence
        );
        // lin の節の本体 (ev_clause が dup するので、入れる前に断る)
        let ev2 = push(&mut ctx, 1, 1);
        let lin = ctx.heap_mut().alloc_data(0, 1, 0, true);
        assert_eq!(
            ctx.ev_set_clause(ev2, 0, lin).unwrap_err(),
            HeapError::BadEvidence
        );
        assert_eq!(ctx.ev_clause(ev2, 0).unwrap(), UNIT);
        ctx.heap_mut().decref(lin).unwrap();
        // エフェクトの番号の上限 (tag は番号 + 1 なので 65535 は作れない)
        assert_eq!(
            ctx.ev_new(u16::MAX, 1, UNIT, UNIT).unwrap_err(),
            HeapError::BadEvidence
        );
        // 合う evidence がない
        assert_eq!(ctx.ev_lookup(7).unwrap_err(), HeapError::NoHandler);
    }
}

/// `evv` の長さの上限が 0 なら、空のリストにも節点を足せない (docs/spec/object-model.md の「実行文脈 `Ctx`」)。
fn a_push_past_the_evv_limit_is_bad_evidence<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let control = Arc::new(Control::new(Limits {
        max_evv: 0,
        ..Limits::default()
    }));
    let mut ctx = Ctx::<M>::new(control, host(&Arc::default()));
    unsafe {
        let ev = ctx.ev_new(0, 1, UNIT, UNIT).unwrap();
        assert_eq!(ctx.evv_push(UNIT, ev).unwrap_err(), HeapError::BadEvidence);
        // 確かめは確保の前なので、ev は呼ぶ側が持ったままである
        ctx.heap_mut().decref(ev).unwrap();
    }
    assert!(ctx.check_finished());
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// `w` のリストの節点と evidence を先頭から並べる。節点は `field(node, 0)` が evidence、`field(node, 1)` が次である。
fn nodes<M: Mode>(ctx: &Ctx<M>, mut list: Word) -> Vec<(Word, Word)> {
    let mut nodes = Vec::new();
    while list != UNIT {
        let ev = unsafe { ctx.heap().field(list, 0) }.unwrap();
        nodes.push((list, ev));
        list = unsafe { ctx.heap().field(list, 1) }.unwrap();
    }
    nodes
}

/// `ev_lookup` と `evv_mask` は、たどった `evv` の節点を `evidence_steps` に数える。
fn lookups_and_masks_count_the_list_nodes_they_visit<M: Mode>() {
    let mut ctx = ctx::<M>();
    let evs: Vec<Word> = [0, 1, 1]
        .into_iter()
        .map(|e| push(&mut ctx, e, 1))
        .collect();
    unsafe {
        let found = ctx.ev_lookup(0).unwrap();
        ctx.heap_mut().decref(found).unwrap();
        if M::CHECKED {
            assert_eq!(ctx.stats().evidence_steps, 3);
        }
        let list = ctx.evv_get().unwrap();
        let masked = ctx.evv_mask(&[item(1, 0, 1)], list).unwrap();
        if M::CHECKED {
            assert_eq!(ctx.stats().evidence_steps, 4);
        }
        ctx.heap_mut().decref(masked).unwrap();
        ctx.heap_mut().decref(list).unwrap();
        for e in evs {
            ctx.heap_mut().decref(e).unwrap();
        }
    }
    clear(&mut ctx);
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// evv は先頭から 0、1、1、0 のエフェクトの evidence である。各項目のエフェクトについて、先頭から合う evidence を
/// 項目の数だけ飛ばし、飛ばした最後の節点の前で残した節点だけを写す。
fn a_mask_skips_the_first_matches_of_each_item<M: Mode>() {
    let mut ctx = ctx::<M>();
    let evs: Vec<Word> = [0, 1, 1, 0]
        .into_iter()
        .map(|e| push(&mut ctx, e, 1))
        .collect();
    unsafe {
        let list = ctx.evv_get().unwrap();
        let original = nodes(&ctx, list);
        let distinct = ctx.evv_mask(&[item(0, 0, 1), item(1, 0, 1)], list).unwrap();
        assert_eq!(distinct, original[2].0);
        let twice = ctx.evv_mask(&[item(1, 0, 2)], list).unwrap();
        let masked = nodes(&ctx, twice);
        assert_eq!(
            masked.iter().map(|&(_, ev)| ev).collect::<Vec<_>>(),
            [original[0].1, original[3].1]
        );
        assert_ne!(masked[0].0, original[0].0);
        assert_eq!(masked[1].0, original[3].0);
        if M::CHECKED {
            assert_eq!(ctx.stats().evidence_steps, 2 + 3);
        }
        for w in [distinct, twice, list] {
            ctx.heap_mut().decref(w).unwrap();
        }
        for e in evs {
            ctx.heap_mut().decref(e).unwrap();
        }
    }
    clear(&mut ctx);
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// evv は先頭から 0、1、1、0、1 のエフェクトの evidence である。エフェクト 1 の先頭の1つを残し、次の1つを除く。
fn a_mask_keeps_the_first_matches_before_it_skips<M: Mode>() {
    let mut ctx = ctx::<M>();
    // `push` は先頭に足すので、後ろから足す
    let evs: Vec<Word> = [0, 1, 1, 0, 1]
        .into_iter()
        .rev()
        .map(|e| push(&mut ctx, e, 1))
        .collect();
    unsafe {
        let list = ctx.evv_get().unwrap();
        let original = nodes(&ctx, list);
        let masked = ctx.evv_mask(&[item(1, 1, 1)], list).unwrap();
        let copied = nodes(&ctx, masked);
        assert_eq!(
            copied.iter().map(|&(_, ev)| ev).collect::<Vec<_>>(),
            [original[0].1, original[1].1, original[3].1, original[4].1]
        );
        assert_ne!(copied[0].0, original[0].0);
        assert_ne!(copied[1].0, original[1].0);
        assert_eq!(copied[2].0, original[3].0);
        if M::CHECKED {
            assert_eq!(ctx.stats().evidence_steps, 3);
        }
        for w in [masked, list] {
            ctx.heap_mut().decref(w).unwrap();
        }
        for e in evs {
            ctx.heap_mut().decref(e).unwrap();
        }
    }
    clear(&mut ctx);
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// 同じエフェクトの項目が2つあれば、先の項目だけを使う。
fn a_second_item_of_the_same_effect_is_ignored<M: Mode>() {
    let mut ctx = ctx::<M>();
    let evs: Vec<Word> = [1, 1].into_iter().map(|e| push(&mut ctx, e, 1)).collect();
    unsafe {
        let list = ctx.evv_get().unwrap();
        let original = nodes(&ctx, list);
        let masked = ctx.evv_mask(&[item(1, 0, 1), item(1, 0, 5)], list).unwrap();
        assert_eq!(masked, original[1].0);
        for w in [masked, list] {
            ctx.heap_mut().decref(w).unwrap();
        }
        for e in evs {
            ctx.heap_mut().decref(e).unwrap();
        }
    }
    clear(&mut ctx);
    assert_eq!(super::mortal(ctx.heap()), 0);
}

fn a_mask_that_skips_more_evidence_than_the_list_holds<M: Mode>() {
    a_refused_mask::<M>(&[item(1, 0, 2)]);
}

fn a_mask_that_keeps_and_skips_more_evidence_than_the_list_holds<M: Mode>() {
    a_refused_mask::<M>(&[item(1, 1, 1)]);
}

/// evv は先頭から 1、0 のエフェクトの evidence で、`items` には合う evidence が足りない。
fn a_refused_mask<M: Mode>(items: &[MaskItem]) {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    let _e0 = push(&mut ctx, 0, 1);
    let _e1 = push(&mut ctx, 1, 1);
    unsafe {
        let list = ctx.evv_get().unwrap();
        let before = super::mortal(ctx.heap());
        assert_eq!(ctx.evv_mask(items, list).unwrap_err(), HeapError::NoHandler);
        // 確保の前に断るので、写しは残らず、たどった数も数えない
        assert_eq!(super::mortal(ctx.heap()), before);
        assert_eq!(ctx.stats().evidence_steps, 0);
    }
}

/// 輪になった evv をたどる `ev_lookup` と `evv_mask` は、小さい上限で BadEvidence になって止まる。
fn a_cyclic_evv_stops_at_the_limit<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let control = Arc::new(Control::new(Limits {
        max_evv: 8,
        ..Limits::default()
    }));
    let mut ctx = Ctx::<M>::new(control, host(&Arc::default()));
    let _e0 = push(&mut ctx, 0, 1);
    unsafe {
        let list = ctx.evv_get().unwrap();
        // 節点の next を自分自身に向けて輪にする (前提を崩すテストなので raw で書く)
        super::raw::set_slot(list, 1, list);
        ctx.heap_mut().decref(list).unwrap();
        assert_eq!(ctx.ev_lookup(5).unwrap_err(), HeapError::BadEvidence);
        let list = ctx.evv_get().unwrap();
        assert_eq!(
            ctx.evv_mask(&[item(5, 0, 1)], list).unwrap_err(),
            HeapError::BadEvidence
        );
    }
}

/// handle の中で perform して、その場で戻る (in-place の節)。
fn perform_in_place<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 3, 1);
    unsafe {
        let clause = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.ev_set_clause(ev, 0, clause).unwrap();
        // perform: ev_lookup + ev_clause + 節の呼び出し (ここでは節を読むだけ)
        let found = ctx.ev_lookup(3).unwrap();
        let c = ctx.ev_clause(found, 0).unwrap();
        assert_eq!(c, clause);
        ctx.heap_mut().decref(c).unwrap();
        ctx.heap_mut().decref(found).unwrap();
        // handle の終わり: evv を外側に戻す
        let outer = ctx.ev_evv(ev).unwrap();
        ctx.evv_set(outer).unwrap();
        ctx.heap_mut().decref(ev).unwrap();
    }
    assert!(ctx.check_finished());
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// yield して継続を組み立て、再開する1往復。
fn yield_and_resume_once<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 0, 1);
    unsafe {
        let body = ctx.heap_mut().alloc_data(0, 1, 0, false);
        let k0 = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.heap_mut().dup(ev).unwrap();
        ctx.yield_to(ev, body, k0).unwrap();
        // 呼び出しのフレームごとに継続を包む
        let k = ctx.yield_cont_take().unwrap();
        let wrapped = ctx.heap_mut().alloc_data(0, 2, 0, false);
        ctx.heap_mut().set_field(wrapped, 1, k).unwrap();
        ctx.yield_cont_set(wrapped).unwrap();
        // prompt$ が受ける
        assert_eq!(ctx.yield_status(ev), 1);
        let k = ctx.yield_cont_take().unwrap();
        let body = ctx.yield_finish().unwrap();
        // 再開: 新しい evidence で節の本体を動かす (ここでは作るだけ)
        let renewed = ctx.ev_renew(ev, UNIT).unwrap();
        for w in [renewed, body, k, ev] {
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    clear(&mut ctx);
    assert!(ctx.check_finished());
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// 物体でない宛先には yield できず、yield の状態はなしのままである。
fn a_yield_to_a_non_object_is_misuse<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        assert_eq!(
            ctx.yield_to(Word::imm(3), UNIT, Word::imm(1)).unwrap_err(),
            HeapError::YieldMisuse
        );
        assert_eq!(
            ctx.yield_cont_set(Word::imm(1)).unwrap_err(),
            HeapError::YieldMisuse
        );
    }
    assert_eq!(ctx.yielding(), 0);
    assert!(ctx.check_finished());
}

/// 断った yield の操作は、yield の状態を変えない。
fn a_refused_yield_operation_keeps_the_state<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    let ev = ctx_evidence(&mut ctx);
    let other = ctx_evidence(&mut ctx);
    unsafe {
        ctx.yield_to(ev, UNIT, Word::imm(1)).unwrap();
        assert_eq!(
            ctx.yield_abort(other, UNIT, UNIT).unwrap_err(),
            HeapError::YieldMisuse
        );
        assert_eq!(ctx.yield_status(ev), 1);
        assert_eq!(ctx.yield_status(other), 3);
        assert_eq!(
            ctx.yield_cont_set(Word::imm(2)).unwrap_err(),
            HeapError::YieldMisuse
        );
        assert_eq!(ctx.yield_finish().unwrap_err(), HeapError::YieldMisuse);
        assert_eq!(ctx.yield_status(ev), 1);
        assert_eq!(ctx.yield_cont_take(), Ok(Word::imm(1)));
        assert_eq!(ctx.yield_finish(), Ok(UNIT));
        ctx.heap_mut().decref(other).unwrap();
    }
    assert!(ctx.check_finished());
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// evidence でない物体と、上限に達したリストには節点を足せない。確かめは確保の前なので、引数は呼ぶ側が持ったままである。
fn a_refused_push_keeps_its_arguments<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let control = Arc::new(Control::new(Limits {
        max_evv: 1,
        ..Limits::default()
    }));
    let mut ctx = Ctx::<M>::new(control, host(&Arc::default()));
    let ev = push(&mut ctx, 0, 1);
    unsafe {
        let closure = ctx.heap_mut().alloc_data(0, 3, 0, false);
        assert_eq!(
            ctx.evv_push(UNIT, closure).unwrap_err(),
            HeapError::BadEvidence
        );
        let full = ctx.evv_get().unwrap();
        ctx.heap_mut().dup(ev).unwrap();
        assert_eq!(ctx.evv_push(full, ev).unwrap_err(), HeapError::BadEvidence);
        for w in [closure, full, ev, ev] {
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    clear(&mut ctx);
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// 共有にした evidence の状態の欄は書けない (docs/spec/lir.md の「evidence」)。欄は元の値のままで、物体も増減しない。
fn the_state_of_a_shared_evidence_is_not_writable<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 0, 1);
    unsafe {
        ctx.ev_set_state(ev, Word::imm(5)).unwrap();
        ctx.heap_mut().share(ev, 1).unwrap();
        let before = super::mortal(ctx.heap());
        assert_eq!(ctx.ev_take_state(ev).unwrap_err(), HeapError::NotWritable);
        assert_eq!(ctx.heap().field(ev, 1), Ok(Word::imm(5)));
        assert_eq!(
            ctx.ev_set_state(ev, Word::imm(6)).unwrap_err(),
            HeapError::NotWritable
        );
        assert_eq!(ctx.heap().field(ev, 1), Ok(Word::imm(5)));
        assert_eq!(super::mortal(ctx.heap()), before);
    }
}

both_modes!(
    a_new_ctx_is_finished,
    poll_reports_each_stop,
    yields_and_continuation_frames_are_counted,
    yield_status_tells_targets_apart,
    misused_yields_are_reported_in_checked_mode,
    evv_get_and_set_swap_the_list,
    dropping_a_ctx_mid_yield,
    lookup_finds_the_innermost_evidence,
    find_returns_the_innermost_evidence_or_none,
    exit_abort_targets_the_dummy,
    clauses_states_and_outer_lists,
    entry_tables_with_a_count_word,
    replacing_a_state_that_reaches_its_evidence,
    bad_evidence_is_reported_in_checked_mode,
    a_push_past_the_evv_limit_is_bad_evidence,
    lookups_and_masks_count_the_list_nodes_they_visit,
    a_mask_skips_the_first_matches_of_each_item,
    a_mask_keeps_the_first_matches_before_it_skips,
    a_second_item_of_the_same_effect_is_ignored,
    a_mask_that_skips_more_evidence_than_the_list_holds,
    a_mask_that_keeps_and_skips_more_evidence_than_the_list_holds,
    a_cyclic_evv_stops_at_the_limit,
    perform_in_place,
    yield_and_resume_once,
    a_yield_to_a_non_object_is_misuse,
    a_refused_yield_operation_keeps_the_state,
    a_refused_push_keeps_its_arguments,
    the_state_of_a_shared_evidence_is_not_writable,
);
