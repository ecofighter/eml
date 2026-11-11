//! `par` の子の `evv` の凍結した部分を作る (docs/spec/vm.md の「`par` の fork と join」)。fork の表は、`fork$E.op` が
//! すべて名前で引けるようになったロードで1回だけ作り、定数と同じく作業用のヒープに不死の物体として作って凍結する。
//! 表と末尾の語は凍結した塊の物体なので、fork のたびに確保せず、どの worker の `evv` からも指せる。

use std::sync::Arc;

use eml_extern::ExternEffect;
use eml_lir::Program;
use eml_rt::layout::{INLINE_SCAN_LIMIT, data_extended, data_first, data_meta};
use eml_rt::{Control, Frozen, Heap, Mode, Word};

use super::Links;
use crate::code::{CompileError, ForkTable, Forks, Index};

/// 読み込んだプログラムのエフェクトのうち、まだ表のないものの表を作る。名前は前のロードの関数からも引く。
/// fork しうるプログラムで名前が引けなければ、コード生成の誤りにする (docs/spec/core-ir.md の「`par` の子の中断」)。
pub(crate) fn extend<M: Mode>(
    program: &Program,
    links: &Links,
    index: &Index,
    forks: &mut Forks,
    region: &mut Option<Arc<Frozen<M>>>,
    control: &Arc<Control>,
) -> Result<(), CompileError> {
    forks.tables.resize(
        index.effects.len().max(forks.tables.len()),
        ForkTable::Missing,
    );
    let mut may_fork = None;
    let mut builder: Option<Heap<M>> = None;
    for (e, info) in program.effects.iter().enumerate() {
        let number = links.effects[e];
        let is_panic = info.name == ExternEffect::Panic.row().name;
        if is_panic {
            forks.panic = Some(number);
        }
        if forks.tables[usize::from(number)] != ForkTable::Missing {
            continue;
        }
        if !info.never_only() {
            forks.tables[usize::from(number)] = ForkTable::Resumable;
            continue;
        }
        let mut entries = Vec::with_capacity(info.operations.len());
        for op in 0..info.operations.len() as u32 {
            let name = eml_lir::fork_name(&program.effects, e as u32, op);
            match index.functions.get(&name) {
                Some(&f) => entries.push(f),
                None if *may_fork.get_or_insert_with(|| program.may_fork()) => {
                    return Err(CompileError::MissingFork { function: name });
                }
                None => break,
            }
        }
        if entries.len() < info.operations.len() {
            continue;
        }
        let heap = builder.get_or_insert_with(|| {
            let mut heap = Heap::with_control(Arc::clone(control));
            if let Some(region) = region {
                heap.attach(Arc::clone(region));
            }
            heap
        });
        let table = table(heap, &entries);
        forks.tables[usize::from(number)] = ForkTable::Table(table);
        if is_panic {
            forks.tail = panic_tail(heap, number, table);
        }
    }
    if let Some(heap) = builder {
        let frozen = heap.freeze().map_err(CompileError::Freeze)?;
        *region = Some(Arc::new(frozen));
    }
    Ok(())
}

/// 走査する語に即値 `&fork$E.op` を操作の順に並べた `Data(tag 0)`。evidence のエントリの表と同じ形である
/// (docs/spec/object-model.md の「evidence の3つの物体」)。
fn table<M: Mode>(heap: &mut Heap<M>, entries: &[u32]) -> Word {
    let n = entries.len();
    // `data_meta`は数の語がないことを前提にし、実行時には確かめないので、ここで止める
    // (docs/spec/vm.mdの「reuse」)
    assert!(
        !data_extended(n, false),
        "an effect with a fork table has fewer than {INLINE_SCAN_LIMIT} operations"
    );
    let first = data_first(n, false);
    // 作業用のヒープの構築中の物体で、ほかの操作の前に走査する語を書き終える (docs/spec/object-model.md の「核の操作」)
    unsafe {
        let w = heap.alloc_immortal(n, data_meta(0, n, false));
        for (i, &f) in entries.iter().enumerate() {
            heap.set_field(w, first + i, Word::imm(i64::from(f)))
                .expect("a fresh table takes its entries");
        }
        w
    }
}

/// `Panic` の fork evidence `[table, 即値 0, ()]` の節点 `[ev, ()]`。`Panic` は mask されないので、深さはいつも 0 で、
/// 子の `evv` の末尾に1つあれば足りる (docs/spec/vm.md の「`par` の fork と join」)。
fn panic_tail<M: Mode>(heap: &mut Heap<M>, panic: u16, table: Word) -> Word {
    let tag = panic + 1;
    // 決まった形の物理的な添字は、形から求める (docs/spec/object-model.mdの「語の位置」)
    const EV: usize = data_first(3, false);
    const NODE: usize = data_first(2, false);
    // 構築中の物体の走査する語を、次の確保の前に書き終える
    unsafe {
        let ev = heap.alloc_immortal(3, data_meta(tag, 3, false));
        for (i, w) in [table, Word::imm(0), Word::UNIT].into_iter().enumerate() {
            heap.set_field(ev, EV + i, w)
                .expect("a fresh fork evidence takes its fields");
        }
        let node = heap.alloc_immortal(2, data_meta(tag, 2, false));
        for (i, w) in [ev, Word::UNIT].into_iter().enumerate() {
            heap.set_field(node, NODE + i, w)
                .expect("a fresh evv node takes its fields");
        }
        node
    }
}
