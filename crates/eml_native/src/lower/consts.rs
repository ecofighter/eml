//! 読み込み時の値のデータの記号。値ごとに、中身だけで名前の決まる8バイトの書けるデータの物を1つ持ち、生成したコードは
//! そこから語を1回のロードで読む。だからコードは、プログラムのほかの部分で変わる番号を持たない
//! (docs/spec/native.md の「値と定数」)。
use std::collections::HashMap;

use cranelift_codegen::ir::{GlobalValue, Type, Value, types};
use cranelift_module::{DataDescription, DataId, Linkage, Module};
use eml_lir::{FnIdx, Program};

use super::apply::{ClosureEntries, ClosureEntry};
use super::function::FnLower;
use super::names;
use crate::abi::Built;
use crate::mem;

/// 中身で表した読み込み時の値1つ。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Datum {
    String(String),
    BigInt(i64),
    /// `float`の定数の箱。ビット列で持つ。
    Float(u64),
    /// 関数の静的なクロージャ (docs/spec/native.md の「関数の値」)。
    Fn(FnIdx),
}

impl Datum {
    /// 値の記号の名前。関数の値は、関数の番号でなく名前で名指す。
    pub fn symbol(&self, program: &Program) -> String {
        match self {
            Datum::String(text) => names::string(text),
            Datum::BigInt(n) => names::big_int(*n),
            Datum::Float(bits) => names::float(*bits),
            Datum::Fn(function) => names::fn_value(&program.function(*function).name),
        }
    }
}

/// moduleが宣言した読み込み時の値。同じ中身の値は記号1つを共有する。
#[derive(Default)]
pub struct Data {
    ids: HashMap<Datum, DataId>,
    /// 最初に使った順である。
    declared: Vec<(Datum, DataId)>,
}

impl Data {
    /// 関数`function`の静的なクロージャの記号。初めて使うときに宣言する。実行系はその行のコードに`c0:<f>`を書くので、
    /// 記号と一緒にそのエントリも宣言する (docs/spec/native.md の「関数の値」)。
    pub(super) fn fn_value<M: Module>(
        &mut self,
        module: &mut M,
        program: &Program,
        closures: &mut ClosureEntries,
        function: FnIdx,
    ) -> DataId {
        let entry = ClosureEntry {
            function,
            captures: 0,
        };
        closures.id(module, program, entry);
        self.id(module, program, Datum::Fn(function))
    }

    /// `datum`の記号。初めて使うときに宣言する。
    fn id<M: Module>(&mut self, module: &mut M, program: &Program, datum: Datum) -> DataId {
        if let Some(&id) = self.ids.get(&datum) {
            return id;
        }
        // 名前の接頭辞は種類ごとに違い、関数の`f:`やクロージャのエントリの`c<k>:`とも重ならないので、宣言が食い違うことはない
        let id = module
            .declare_data(&datum.symbol(program), Linkage::Local, true, false)
            .unwrap_or_else(|error| panic!("a data symbol cannot be declared: {error}"));
        self.ids.insert(datum.clone(), id);
        self.declared.push((datum, id));
        id
    }

    /// 宣言したどの記号も、0で埋めた8バイトの、8バイトにそろえた物として定義し、値と記号を返す。
    pub fn define<M: Module>(self, module: &mut M) -> Built<Vec<(Datum, DataId)>> {
        for &(_, id) in &self.declared {
            let mut description = DataDescription::new();
            description.define_zeroinit(8);
            description.set_align(8);
            module.define_data(id, &description)?;
        }
        Ok(self.declared)
    }
}

impl<M: Module> FnLower<'_, '_, M> {
    /// `datum`の語を型`ty`で読む。
    pub(super) fn datum_word(&mut self, datum: Datum, ty: Type) -> Value {
        let id = self.data.id(self.module, self.callees.program, datum);
        let slot = self.global_value(id);
        mem::data_word(&mut self.b, slot, ty)
    }

    /// 関数`function`の静的なクロージャの語。
    pub(super) fn fn_value(&mut self, function: FnIdx) -> Value {
        let id = self
            .data
            .fn_value(self.module, self.callees.program, self.closures, function);
        let slot = self.global_value(id);
        mem::data_word(&mut self.b, slot, types::I64)
    }

    /// `id`の記号をこの関数から指す値。関数ごとに1回だけ取り込む。
    fn global_value(&mut self, id: DataId) -> GlobalValue {
        if let Some(&slot) = self.slots.get(&id) {
            return slot;
        }
        let slot = self.module.declare_data_in_func(id, self.b.func);
        self.slots.insert(id, slot);
        slot
    }
}
