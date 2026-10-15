//! 定数を作業用のヒープに不死の物体として作り、凍結する (docs/spec/vm.md の「`CodeBase` と `FnEntry`」)。塊の物体は
//! 検査モードでも参照を数えないので、VM は定数の語を写すだけで使える。

use std::collections::HashMap;
use std::sync::Arc;

use eml_rt::{Control, Frozen, Heap, Kind, Meta, Mode, Word};

use crate::code::CompileError;

pub(crate) struct Constants<M: Mode> {
    heap: Heap<M>,
    words: Vec<Word>,
    /// 生の整数の値ごとの番号。同じ値を表に2度置かない。
    raws: HashMap<i64, u32>,
}

impl<M: Mode> Constants<M> {
    /// 塊のページは、実行のヒープと同じ `Control` の確保量に数える。
    pub(crate) fn new(control: Arc<Control>) -> Constants<M> {
        Constants {
            heap: Heap::with_control(control),
            words: Vec::new(),
            raws: HashMap::new(),
        }
    }

    fn push(&mut self, w: Word) -> u32 {
        self.words.push(w);
        u32::try_from(self.words.len() - 1).expect("the constants fit in u32 positions")
    }

    pub(crate) fn string(&mut self, text: &str) -> u32 {
        let w = self.heap.alloc_string_immortal(text);
        self.push(w)
    }

    /// 生の整数。`LoadRaw` の `i32` に収まらない `Int` の定数に使う。
    pub(crate) fn raw(&mut self, n: i64) -> u32 {
        if let Some(&index) = self.raws.get(&n) {
            return index;
        }
        let index = self.push(Word::from_bits(n as usize));
        self.raws.insert(n, index);
        index
    }

    /// `tobj` の `Int`。63 ビットに収まれば即値、収まらなければ不死の `BigInt` にする。
    pub(crate) fn boxed_int(&mut self, n: i64) -> u32 {
        let w = match Word::int(n) {
            Some(w) => w,
            // 作業用のヒープの構築中の物体で、ほかの操作の前に値の語を書き終える (docs/spec/object-model.md の「核の操作」)
            None => unsafe {
                let w = self
                    .heap
                    .alloc_immortal(1, Meta::new(0, 0, Kind::BigInt, false));
                self.heap
                    .set_raw(w, 0, n as u64)
                    .expect("a fresh BigInt takes its value");
                w
            },
        };
        self.push(w)
    }

    pub(crate) fn freeze(self) -> Result<(Arc<Frozen<M>>, Vec<Word>), CompileError> {
        let region = self.heap.freeze().map_err(CompileError::Freeze)?;
        Ok((Arc::new(region), self.words))
    }
}
