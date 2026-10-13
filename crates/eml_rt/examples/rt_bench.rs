//! `eml_rt` の速いモードの計測 (docs/implementation/benchmarks.md)。`bench/rt.sh` が release ビルドで走らせ、命令の数を
//! 測る。reuse analysis はまだないので、reuse の列は手で組む。reuse の効果の見込みを、パスを作る前に数字で持つためである。

use std::hint::black_box;

use eml_rt::{Fast, Heap, Kind, Meta, Word};

const CONS: Meta = Meta::new(1, 2, Kind::Data, false);
const CELLS: usize = 1_000_000;
const ROUNDS: usize = 10;

fn main() {
    let case = std::env::args()
        .nth(1)
        .expect("a case: empty, list, box-list, dup, map, map-reuse");
    match case.as_str() {
        "empty" => {}
        "list" => list(),
        "box-list" => box_list(),
        "dup" => dup(),
        "map" => map(false),
        "map-reuse" => map(true),
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
