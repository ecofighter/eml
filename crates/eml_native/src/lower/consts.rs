//! 読み込み時の値と`release`の`keep`のバイトのデータの記号。値ごとに、中身だけで名前の決まる書けるデータの物を1つ持ち、
//! 生成したコードはそこから語を1回のロードで読むか、マスクならそのアドレスを取る。`keep`のバイトは、コンパイルの時点で
//! 中身を決めた読み取り専用の物で、生成したコードはそのアドレスだけを取る。だからコードは、プログラムのほかの部分で変わる
//! 番号を持たない (docs/spec/native.mdの「値と定数」)。
use std::collections::HashMap;

use cranelift_codegen::ir::{GlobalValue, Type, Value, types};
use cranelift_module::{DataDescription, DataId, Linkage, Module};
use eml_lir::syntax::{effect_text, mask_text};
use eml_lir::{FnIdx, MaskItem, Program};
use eml_nrt::image::mask_bytes;

use super::apply::{ClosureEntries, ClosureEntry};
use super::function::FnLower;
use super::names;
use crate::abi::{Built, EntrySig};
use crate::mem;

/// 中身で表したデータの物1つ。`Keep`のほかは読み込み時の値である。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Datum {
    String(String),
    BigInt(i64),
    /// `float`の定数の箱。ビット列で持つ。
    Float(u64),
    /// 関数の静的なクロージャ (docs/spec/native.md の「関数の値」)。
    Fn(FnIdx),
    /// 長さ0の`MutArray`の不死の物体 (docs/spec/native.md の「値と定数」)。
    EmptyMutArray,
    /// エフェクトのマシンの番号。値は`Program.effects`でのエフェクトの添字を持ち、slotはそのマシンの番号を持つ
    /// (docs/spec/native.mdの「エフェクト」)。
    Effect(u32),
    /// `evv_mask`の項目。LIRの順で、エフェクトは`Program.effects`の添字である。
    Mask(Vec<MaskItem>),
    /// `sig_word`が書けないエントリの署名に、ホストが与える語 (docs/spec/native.mdの「`Image`」)。
    Sig(EntrySig),
    /// `release`の走査する語ごとの`keep`のバイト。残す語は1、ほかは0である (docs/spec/native.mdの「データ」)。
    Keep(Vec<bool>),
}

impl Datum {
    /// 値の記号の名前。関数の値は、関数の番号でなく名前で名指す。
    pub fn symbol(&self, program: &Program) -> String {
        match self {
            Datum::String(text) => names::string(text),
            Datum::BigInt(n) => names::big_int(*n),
            Datum::Float(bits) => names::float(*bits),
            Datum::Fn(function) => names::fn_value(&program.function(*function).name),
            Datum::EmptyMutArray => names::EMPTY_MUT_ARRAY.to_string(),
            Datum::Effect(effect) => names::effect(&effect_text(&program.effects, *effect)),
            Datum::Mask(items) => names::mask(&mask_text(&program.effects, items)),
            Datum::Sig(sig) => names::sig(sig),
            Datum::Keep(keep) => names::keep(keep),
        }
    }

    /// データの物のバイト数。1語か、マスクの項目か、`keep`のバイトを8バイトの倍数に切り上げた数である。`keep`は1語以上
    /// なので、走査する語のない`release`にもnullでないアドレスを渡せる。
    pub fn bytes(&self) -> usize {
        match self {
            Datum::Mask(items) => mask_bytes(items.len()),
            Datum::Keep(keep) => keep.len().div_ceil(8).max(1) * 8,
            _ => 8,
        }
    }

    /// ローダーが値を書く物か。`keep`のバイトだけは、moduleが中身を定義し、ローダーは書かない。
    fn loaded(&self) -> bool {
        !matches!(self, Datum::Keep(_))
    }
}

/// moduleが宣言したデータの物。同じ中身の物は記号1つを共有する。
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
        // 名前の接頭辞は種類ごとに違い、関数の`f:`やクロージャのエントリの`c<k>:`とも重ならないので、宣言が食い違うことはない。
        // 書かない物を書けない物として宣言するので、JITはそれを読み取り専用のページに置く
        let id = module
            .declare_data(
                &datum.symbol(program),
                Linkage::Local,
                datum.loaded(),
                false,
            )
            .unwrap_or_else(|error| panic!("a data symbol cannot be declared: {error}"));
        self.ids.insert(datum.clone(), id);
        self.declared.push((datum, id));
        id
    }

    /// 宣言したどの記号も、8バイトにそろえた、そのバイト数の物として定義する。読み込み時の値は0で埋め、`keep`のバイトは
    /// 中身を0を足したバイトで書く。ローダーが書く読み込み時の値と、その記号を返す。
    pub fn define<M: Module>(self, module: &mut M) -> Built<Vec<(Datum, DataId)>> {
        for (datum, id) in &self.declared {
            let mut description = DataDescription::new();
            match datum {
                Datum::Keep(keep) => {
                    let mut bytes = vec![0u8; datum.bytes()];
                    for (byte, &kept) in bytes.iter_mut().zip(keep) {
                        *byte = u8::from(kept);
                    }
                    description.define(bytes.into_boxed_slice());
                }
                _ => description.define_zeroinit(datum.bytes()),
            }
            description.set_align(8);
            module.define_data(*id, &description)?;
        }
        Ok(self
            .declared
            .into_iter()
            .filter(|(datum, _)| datum.loaded())
            .collect())
    }
}

impl<M: Module> FnLower<'_, '_, M> {
    /// `datum`の語を型`ty`で読む。
    pub(super) fn datum_word(&mut self, datum: Datum, ty: Type) -> Value {
        let id = self.data.id(self.module, self.callees.program, datum);
        let slot = self.global_value(id);
        mem::data_word(&mut self.b, slot, ty)
    }

    /// `datum`のデータの物のアドレス。
    pub(super) fn datum_address(&mut self, datum: Datum) -> Value {
        let id = self.data.id(self.module, self.callees.program, datum);
        let slot = self.global_value(id);
        mem::data_address(&mut self.b, slot)
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
