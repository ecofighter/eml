//! 定数を作業用のヒープに不死の物体として作り、凍結する (docs/spec/vm.md の「`CodeBase` と `FnEntry`」)。塊の物体は
//! 検査モードでも参照を数えないので、VM は定数の語を写すだけで使える。定数は中身で引き、前に読み込んだ定数は同じ番号を
//! 使う。新しい物体があるときだけ、今の先頭の塊を親にした新しい塊を作る (docs/spec/vm.md の「追記」)。

use std::collections::HashMap;
use std::sync::Arc;

use eml_lir::FloatBits;
use eml_rt::{Control, Frozen, Heap, Kind, Meta, Mode, Word};

use crate::code::{CompileError, Index};

pub(crate) struct Constants<'c, M: Mode> {
    words: &'c mut Vec<Word>,
    strings: &'c mut HashMap<String, u32>,
    raws: &'c mut HashMap<i64, u32>,
    boxed: &'c mut HashMap<i64, u32>,
    boxed_floats: &'c mut HashMap<FloatBits, u32>,
    region: &'c Option<Arc<Frozen<M>>>,
    control: &'c Arc<Control>,
    /// 新しい物体を作る作業用のヒープ。最初の物体を作るときに作る。
    builder: Option<Heap<M>>,
}

impl<'c, M: Mode> Constants<'c, M> {
    pub(crate) fn new(
        words: &'c mut Vec<Word>,
        index: &'c mut Index,
        region: &'c Option<Arc<Frozen<M>>>,
        control: &'c Arc<Control>,
    ) -> Constants<'c, M> {
        Constants {
            words,
            strings: &mut index.strings,
            raws: &mut index.raws,
            boxed: &mut index.boxed,
            boxed_floats: &mut index.boxed_floats,
            region,
            control,
            builder: None,
        }
    }

    fn push(&mut self, w: Word) -> u32 {
        self.words.push(w);
        u32::try_from(self.words.len() - 1).expect("the constants fit in u32 positions")
    }

    /// 塊のページは、実行のヒープと同じ `Control` の確保量に数える。今の先頭の塊を付けてから物体を作るので、凍結した
    /// 塊はそれを親に持ち、鎖が伸びる。
    fn heap(&mut self) -> &mut Heap<M> {
        let (region, control) = (self.region, self.control);
        self.builder.get_or_insert_with(|| {
            let mut heap = Heap::with_control(Arc::clone(control));
            if let Some(region) = region {
                heap.attach(Arc::clone(region));
            }
            heap
        })
    }

    pub(crate) fn string(&mut self, text: &str) -> u32 {
        if let Some(&index) = self.strings.get(text) {
            return index;
        }
        let w = self.heap().alloc_string_immortal(text);
        let index = self.push(w);
        self.strings.insert(text.to_string(), index);
        index
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
        if let Some(&index) = self.boxed.get(&n) {
            return index;
        }
        let w = match Word::int(n) {
            Some(w) => w,
            // 作業用のヒープの構築中の物体で、ほかの操作の前に値の語を書き終える (docs/spec/object-model.md の「核の操作」)
            None => unsafe {
                let heap = self.heap();
                let w = heap.alloc_immortal(1, Meta::new(0, 0, Kind::BigInt, false));
                heap.set_raw(w, 0, n as u64)
                    .expect("a fresh BigInt takes its value");
                w
            },
        };
        let index = self.push(w);
        self.boxed.insert(n, index);
        index
    }

    /// `tobj` の `Float`。いつも不死の `Float` の物体にする。表はビット列で引くので、`0.0` と `-0.0` を別の定数にする。
    pub(crate) fn boxed_float(&mut self, x: FloatBits) -> u32 {
        if let Some(&index) = self.boxed_floats.get(&x) {
            return index;
        }
        // 作業用のヒープの構築中の物体で、ほかの操作の前に値の語を書き終える (docs/spec/object-model.md の「核の操作」)
        let w = unsafe {
            let heap = self.heap();
            let w = heap.alloc_immortal(1, Meta::new(0, 0, Kind::Float, false));
            heap.set_raw(w, 0, x.0)
                .expect("a fresh Float takes its value");
            w
        };
        let index = self.push(w);
        self.boxed_floats.insert(x, index);
        index
    }

    /// 新しい物体があれば凍結した塊にして返す。なければ塊を作らない。塊は使う大きさの級ごとに少なくとも1ページを
    /// 持つためである (docs/spec/object-model.md の「凍結した領域」)。
    pub(crate) fn freeze(self) -> Result<Option<Arc<Frozen<M>>>, CompileError> {
        self.builder
            .map(|heap| heap.freeze().map(Arc::new).map_err(CompileError::Freeze))
            .transpose()
    }
}
