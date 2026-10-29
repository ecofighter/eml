//! `eml_rt` の速いモードの計測 (docs/implementation/benchmarks.md)。`bench/rt.sh` が release ビルドで走らせ、命令の数を
//! 測る。reuse の列は手で組む。VM の reuse の命令の速い道と同じ基本操作を、`eml_rt` だけで測るためである。

use std::hint::black_box;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use eml_rt::{
    Buffering, Control, Ctx, Fast, Heap, HostRef, Kind, Limits, MaskItem, Meta, StdHost, Word,
};

const CONS: Meta = Meta::new(1, 2, Kind::Data, false);
const CELLS: usize = 1_000_000;
const ROUNDS: usize = 10;
const APPENDS: usize = 10_000_000;
const PRIMITIVES: usize = 10_000_000;
const HANDLERS: usize = 1_000_000;
const POLLS: usize = 100_000_000;
const UNIT: Word = Word::imm(0);

fn main() {
    let case = std::env::args().nth(1).expect(
        "a case: empty, list, box-list, dup, free, map, map-reuse, append, perform, \
             perform-deep, handle, yield, mask, poll",
    );
    match case.as_str() {
        "empty" => {}
        "list" => list(),
        "box-list" => box_list(),
        "dup" => dup(),
        "free" => free(),
        "map" => map(false),
        "map-reuse" => map(true),
        "append" => append(),
        "perform" => perform(&[0], 0),
        "perform-deep" => perform(&[1, 2, 3, 4, 5, 6, 7, 8], 1),
        "handle" => handle(),
        "yield" => yield_once(),
        "mask" => mask(),
        "poll" => poll(),
        _ => panic!("an unknown case {case}"),
    }
}

fn build(heap: &mut Heap<Fast>, n: usize) -> Word {
    let mut l = Word::imm(0);
    for i in 0..n {
        // SAFETY: 確保した cell の2つの語を、ほかのヒープの操作の前に書く。cell と l はここで確保した生きた物体である。
        let cell = unsafe {
            let cell = heap.alloc(2, CONS);
            heap.set_field(cell, 0, Word::imm(i as i64)).unwrap();
            heap.set_field(cell, 1, l).unwrap();
            cell
        };
        l = cell;
    }
    l
}

/// cons セルのリストを作って捨てる。
fn list() {
    let mut heap = Heap::<Fast>::new();
    for _ in 0..ROUNDS {
        let l = build(&mut heap, CELLS);
        // SAFETY: l は build が返した生きたリストで、参照を1つ持っている。
        unsafe { heap.decref(black_box(l)) }.unwrap();
    }
}

// 値の語は読まないが、ペイロードの語の数を cons セルと同じ2語 (値と次の箱) にそろえるために持つ
#[expect(dead_code)]
enum BoxList {
    Nil,
    Cons(i64, Box<BoxList>),
}

/// 同じことを `std::alloc` を直接使う `Box` のリストで行う。比べる基準である。
fn box_list() {
    for _ in 0..ROUNDS {
        let mut l = BoxList::Nil;
        for i in 0..CELLS as i64 {
            l = BoxList::Cons(i, Box::new(l));
        }
        // 再帰する Drop でスタックを溢れさせないように、先頭から1つずつ外す
        let mut cur = black_box(l);
        while let BoxList::Cons(_, next) = cur {
            cur = *next;
        }
    }
}

/// 局所の物体の `dup` と `decref` の繰り返し。
fn dup() {
    let mut heap = Heap::<Fast>::new();
    // SAFETY: x は確保して2つの語を書いた生きた物体である。ループ内の dup と decref は対で、最後の decref まで x の
    // 参照は1つ以上残る。
    unsafe {
        let x = heap.alloc(2, CONS);
        heap.set_field(x, 0, Word::UNIT).unwrap();
        heap.set_field(x, 1, Word::UNIT).unwrap();
        for _ in 0..CELLS * ROUNDS {
            heap.dup(black_box(x)).unwrap();
            heap.decref(black_box(x)).unwrap();
        }
        heap.decref(x).unwrap();
    }
}

/// cons セルを1つ確保して2つの語を書き、`decref` で空けることの繰り返し。`list` の解放は解放の連鎖の中の `release_ref`
/// を通るので、`decref` が一意な物体を空ける道はこの計測だけが測る。
fn free() {
    let mut heap = Heap::<Fast>::new();
    for _ in 0..CELLS * ROUNDS {
        // SAFETY: 確保した cell の2つの語を、ほかのヒープの操作の前に書く。cell はここで確保した生きた物体で、参照を1つ
        // 持っている。
        unsafe {
            let cell = heap.alloc(2, CONS);
            heap.set_field(cell, 0, Word::UNIT).unwrap();
            heap.set_field(cell, 1, Word::UNIT).unwrap();
            heap.decref(black_box(cell)).unwrap();
        }
    }
}

/// `map (+1)` に当たる作り直し (逆順のリストを作る)。`reuse` なら、空けた cons の箱を次の cons に使う。
fn map(reuse: bool) {
    let mut heap = Heap::<Fast>::new();
    let mut l = build(&mut heap, CELLS);
    for _ in 0..ROUNDS {
        let mut acc = Word::imm(0);
        let mut cur = l;
        while cur.is_ptr() {
            // SAFETY: cur は生きた cons で、ループはその参照を1つ持つ。最初は build が返した l の参照で、その後は前の
            // cons の tail を keep で受け継いだ参照である。release の後は cur を読まない。head は即値である。作った cell は
            // 2つの語を、ほかのヒープの操作の前に書く。
            unsafe {
                let head = heap.field(cur, 0).unwrap();
                let tail = heap.field(cur, 1).unwrap();
                let cell = if reuse {
                    let token = heap.release_reuse(cur, 1, &[false, true]).unwrap();
                    heap.con_at(token, 2, CONS).unwrap().0
                } else {
                    heap.release(cur, 1, &[false, true]).unwrap();
                    heap.alloc(2, CONS)
                };
                heap.set_field(cell, 0, Word::imm(head.imm_value() + 1))
                    .unwrap();
                heap.set_field(cell, 1, acc).unwrap();
                acc = cell;
                cur = tail;
            }
        }
        l = acc;
    }
    // SAFETY: l は最後の round が作った生きたリストで、参照を1つ持っている。
    unsafe { heap.decref(black_box(l)) }.unwrap();
}

/// 一意な文字列に 1 バイトずつ足していく。その場の追記と、倍の容量での作り直しの費用である。
fn append() {
    let mut heap = Heap::<Fast>::new();
    let mut s = heap.alloc_string("");
    for _ in 0..APPENDS {
        // SAFETY: s は直前の追記が返した生きた文字列で、参照を1つ持っている。
        s = unsafe { heap.string_push(black_box(s), "x") }.unwrap();
    }
    // SAFETY: 同上。
    unsafe { heap.decref(s) }.unwrap();
}

fn ctx() -> Ctx<Fast> {
    let host = StdHost::new(
        Box::new(io::sink()),
        Box::new(io::sink()),
        PathBuf::new(),
        Buffering::Block,
    );
    Ctx::new(
        Arc::new(Control::new(Limits::default())),
        HostRef::new(Arc::new(host)),
    )
}

/// `evv` の先頭に、エフェクト `effect` の操作1つの evidence を足す。エントリは関数の値 (即値) である。
fn push(ctx: &mut Ctx<Fast>, effect: u16) {
    // SAFETY: evv_get と ev_new が返した参照をそのまま次の操作に渡し、ほかの語には触れない。
    unsafe {
        let outer = ctx.evv_get().unwrap();
        let ev = ctx.ev_new(effect, 1, UNIT, outer).unwrap();
        ctx.ev_set_clause(ev, 0, Word::imm(1)).unwrap();
        let list = ctx.evv_get().unwrap();
        let list = ctx.evv_push(list, ev).unwrap();
        ctx.evv_set(list).unwrap();
    }
}

/// `effects` の evidence を順に足した `evv` で、エフェクト `target` に `perform` してその場で戻る。
fn perform(effects: &[u16], target: u16) {
    let mut ctx = ctx();
    for &e in effects {
        push(&mut ctx, e);
    }
    for _ in 0..PRIMITIVES {
        // SAFETY: target の evidence は evv にある。ev_lookup と ev_clause が返した参照を1つずつ手放す。
        unsafe {
            let ev = ctx.ev_lookup(black_box(target)).unwrap();
            let clause = ctx.ev_clause(ev, 0).unwrap();
            ctx.heap_mut().decref(black_box(clause)).unwrap();
            ctx.heap_mut().decref(ev).unwrap();
        }
    }
    // SAFETY: evv の参照は Ctx が持つ。
    unsafe { ctx.evv_set(UNIT) }.unwrap();
}

/// handler を設けて外す。
fn handle() {
    let mut ctx = ctx();
    for _ in 0..HANDLERS {
        // SAFETY: ev は ev_new が返した参照と、dup した参照の2つを持つ。1つは evv_push が節点に移し、もう1つは
        // 最後に手放す。evv_get と ev_take_evv が返したリストの参照は、evv_push と evv_set に渡す。コンパイラの
        // `prompt$` と同じく、外側の evv は dup せずに evidence から取り出す。
        unsafe {
            let outer = ctx.evv_get().unwrap();
            let ev = ctx.ev_new(0, 1, UNIT, outer).unwrap();
            ctx.ev_set_clause(ev, 0, Word::imm(1)).unwrap();
            ctx.heap_mut().dup(ev).unwrap();
            let list = ctx.evv_get().unwrap();
            let list = ctx.evv_push(list, ev).unwrap();
            ctx.evv_set(list).unwrap();
            let outer = ctx.ev_take_evv(black_box(ev)).unwrap();
            ctx.evv_set(outer).unwrap();
            ctx.heap_mut().decref(ev).unwrap();
        }
    }
}

/// yield して継続を1回組み立て直し、再開する1往復。
fn yield_once() {
    let mut ctx = ctx();
    // SAFETY: ev は ev_new が返した参照を持つ。yield_to に渡す参照は dup で作り、yield_finish が手放す。
    unsafe {
        let ev = ctx.ev_new(0, 1, UNIT, UNIT).unwrap();
        for _ in 0..PRIMITIVES {
            ctx.heap_mut().dup(ev).unwrap();
            ctx.yield_to(black_box(ev), Word::imm(2), Word::imm(1))
                .unwrap();
            let k = ctx.yield_cont_take().unwrap();
            ctx.yield_cont_set(black_box(k)).unwrap();
            let k = ctx.yield_cont_take().unwrap();
            black_box(k);
            black_box(ctx.yield_finish().unwrap());
        }
        ctx.heap_mut().decref(ev).unwrap();
    }
}

/// 4つの evidence の `evv` で、2つ目の evidence を飛ばす。
fn mask() {
    let mut ctx = ctx();
    for e in 0..4 {
        push(&mut ctx, e);
    }
    let items = [MaskItem {
        effect: 2,
        keep: 0,
        skip: 1,
    }];
    // SAFETY: list は evv_get が返した参照で、evv_mask は借りるだけである。evv_mask の結果は1回ずつ手放す。
    unsafe {
        let list = ctx.evv_get().unwrap();
        for _ in 0..PRIMITIVES {
            let masked = ctx.evv_mask(black_box(&items), list).unwrap();
            ctx.heap_mut().decref(black_box(masked)).unwrap();
        }
        ctx.heap_mut().decref(list).unwrap();
        ctx.evv_set(UNIT).unwrap();
    }
}

/// 安全点の確かめ。
fn poll() {
    let ctx = ctx();
    for depth in 0..POLLS {
        ctx.poll(black_box(depth)).unwrap();
    }
}
