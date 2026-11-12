//! 構築子、欄、`switch`、箱、文字列の定数 (docs/spec/native.md の「データ」、docs/spec/native.md の「値と定数」)。
//! 欄の位置は`LayoutCtor::shape()`から取り、`mem`の関数で読み書きする。
use cranelift_codegen::ir::{Block, BlockArg, InstBuilder, TrapCode, Type, Value, types};
use cranelift_frontend::Switch;
use cranelift_module::Module;
use eml_lir::{Atom, BlockId, Case, CasePattern, Ctor, LayoutCtor, LirFn, Mark, Repr, Term, VarId};

use super::consts::Datum;
use super::function::FnLower;
use super::kind;
use crate::abi::clif;
use crate::mem;

/// 行き先のブロックの先頭で束縛するcaseの欄。`scrutinee`は`switch`の値で、`ctor`はcaseの構築子である。
#[derive(Clone, Copy)]
pub(super) struct CaseFields<'a> {
    scrutinee: VarId,
    ctor: Ctor,
    fields: &'a [VarId],
}

impl<'a> CaseFields<'a> {
    /// ブロックごとの、そこを行き先とする欄のあるcase。`switch`の行き先に入る辺はその`switch`の1本だけで、行き先の
    /// 番号は`switch`のブロックより大きい (docs/spec/lir.md の「構造の規則」のR2とR3) ので、行き先を下ろすときには
    /// `switch`の値はもう束縛してある。
    pub(super) fn of(function: &'a LirFn) -> Vec<Option<CaseFields<'a>>> {
        let mut cases = vec![None; function.blocks.len()];
        for block in &function.blocks {
            if let Term::Switch {
                scrutinee: Atom::Var(scrutinee),
                layout: Some(layout),
                cases: each,
                default: _,
            } = &block.term
            {
                for case in each {
                    if let CasePattern::Tag(tag) = case.pattern
                        && !case.fields.is_empty()
                    {
                        cases[case.target.0 as usize] = Some(CaseFields {
                            scrutinee: *scrutinee,
                            ctor: Ctor {
                                layout: *layout,
                                tag,
                            },
                            fields: &case.fields,
                        });
                    }
                }
            }
        }
        cases
    }
}

impl<M: Module> FnLower<'_, '_, M> {
    fn layout_ctor(&self, ctor: Ctor) -> &LayoutCtor {
        &self.callees.program.layouts[ctor.layout.0 as usize].constructors[ctor.tag as usize]
    }

    /// `let var = con ctor(args)`。物体をエントリで作り、どの欄もストア1つで書く。`args`の所有は物体へ移る。
    pub(super) fn con(&mut self, var: VarId, ctor: Ctor, args: &[Atom]) {
        let layout_ctor = self.layout_ctor(ctor);
        let shape = layout_ctor.shape();
        let reprs = layout_ctor.fields.clone();
        let values: Vec<Value> = args
            .iter()
            .zip(reprs)
            .map(|(&arg, repr)| self.atom(arg, repr))
            .collect();
        let lin = self.function.mark(var) == Mark::Lin;
        let head = [
            i64::from(ctor.tag),
            i64::from(shape.scanned),
            i64::from(shape.raw),
            i64::from(lin),
        ]
        .map(|n| self.b.ins().iconst(types::I64, n));
        let w = self
            .call_entry("eml_rt_alloc_data", &head)
            .expect("an allocation returns its word");
        for (value, &position) in values.into_iter().zip(&shape.positions) {
            mem::store_field(&mut self.b, w, shape.scanned, position, value);
        }
        self.bind(var, w);
    }

    /// `ctor`の物体`value`の欄を`fields`に束縛する。欄は物体から借りるので、数は変えない。
    fn bind_fields(&mut self, value: VarId, ctor: Ctor, fields: &[VarId]) {
        let shape = self.layout_ctor(ctor).shape();
        let w = self.var(value);
        for (&field, &position) in fields.iter().zip(&shape.positions) {
            let ty = clif(kind(self.function.repr(field)));
            let loaded = mem::load_field(&mut self.b, w, shape.scanned, position, ty);
            self.bind(field, loaded);
        }
    }

    /// `unpack value ctor(fields)`。
    pub(super) fn unpack(&mut self, value: VarId, ctor: Ctor, fields: &[VarId]) {
        self.bind_fields(value, ctor, fields);
    }

    /// ブロック`block`が欄のあるcaseの行き先なら、その欄を読む。
    pub(super) fn case_fields(&mut self, block: BlockId) {
        if let Some(case) = self.cases[block.0 as usize] {
            self.bind_fields(case.scrutinee, case.ctor, case.fields);
        }
    }

    /// `let var = const "..."`。文字列は凍結した領域の不死の物体で、その記号から読む。
    pub(super) fn const_string(&mut self, var: VarId, index: u32) {
        let text = self.callees.program.strings[index as usize].clone();
        let w = self.datum_word(Datum::String(text), types::I64);
        self.bind(var, w);
    }

    /// `let var = box atom`。箱の種類はオペランドのReprで決まる。即値に収まる`int`、`enum`、タグは即値の語で、
    /// 定数の大きい`int`と`float`は読み込み時の値の記号から読む。
    pub(super) fn boxed(&mut self, var: VarId, atom: Atom) {
        let w = match atom {
            Atom::Int(n) if mem::fits_imm(n) => self.b.ins().iconst(types::I64, mem::imm_bits(n)),
            Atom::Int(n) => self.datum_word(Datum::BigInt(n), types::I64),
            Atom::Float(bits) => self.datum_word(Datum::Float(bits.0), types::I64),
            Atom::Tag(n) => self.b.ins().iconst(types::I64, mem::imm_bits(i64::from(n))),
            Atom::Var(v) => {
                let x = self.var(v);
                match self.function.repr(v) {
                    Repr::Int => self.box_int(x),
                    Repr::Enum => mem::make_imm(&mut self.b, x),
                    Repr::Float => self
                        .call_entry("eml_rt_alloc_float", &[x])
                        .expect("an allocation returns its word"),
                    repr => unreachable!("a box of `{repr:?}` does not pass the verifier"),
                }
            }
            Atom::Unit | Atom::Fn(_) => {
                unreachable!("a box of {atom:?} does not pass the verifier")
            }
        };
        self.bind(var, w);
    }

    /// `int`の箱。63ビットに収まれば即値の語で、収まらなければ`BigInt`の物体をエントリで作る (`Heap::alloc_int`の規則)。
    fn box_int(&mut self, n: Value) -> Value {
        let imm = self.b.create_block();
        let big = self.b.create_block();
        let done = self.b.create_block();
        let w = self.b.append_block_param(done, types::I64);
        let fits = mem::imm_round_trips(&mut self.b, n);
        self.b.ins().brif(fits, imm, &[], big, &[]);
        self.b.switch_to_block(imm);
        let small = mem::make_imm(&mut self.b, n);
        self.b.ins().jump(done, &[BlockArg::Value(small)]);
        self.b.switch_to_block(big);
        let boxed = self
            .call_entry("eml_rt_alloc_int", &[n])
            .expect("an allocation returns its word");
        self.b.ins().jump(done, &[BlockArg::Value(boxed)]);
        self.b.switch_to_block(done);
        w
    }

    /// `let var = unbox atom`。箱を読むだけで、所有は受け取らない。`int`と`enum`の箱は即値か物体で、`float`の箱は
    /// いつも物体である。ダミーは値0の`BigInt`なので、どの種類でも0を読む (docs/spec/object-model.md の
    /// 「ダミーの値」)。
    pub(super) fn unbox(&mut self, var: VarId, atom: Atom) {
        let Atom::Var(boxed) = atom else {
            unreachable!("an unbox of a constant does not pass the verifier")
        };
        let w = self.var(boxed);
        let value = match self.function.repr(var) {
            Repr::Float => mem::load_box(&mut self.b, w, types::F64),
            Repr::Int | Repr::Enum => {
                let (object, done) = self.on_immediate(w, types::I64);
                let small = mem::imm_value(&mut self.b, w);
                self.b.ins().jump(done, &[BlockArg::Value(small)]);
                self.b.switch_to_block(object);
                let big = mem::load_box(&mut self.b, w, types::I64);
                self.b.ins().jump(done, &[BlockArg::Value(big)]);
                self.b.switch_to_block(done);
                self.b.block_params(done)[0]
            }
            repr => unreachable!("an unbox to `{repr:?}` does not pass the verifier"),
        };
        self.bind(var, value);
    }

    /// 語`w`が即値かで分かれる。builderを即値の側に置き、物体の側と、型`ty`の引数を1つ持つ合流のブロックを返す。
    fn on_immediate(&mut self, w: Value, ty: Type) -> (Block, Block) {
        let imm = self.b.create_block();
        let object = self.b.create_block();
        let done = self.b.create_block();
        self.b.append_block_param(done, ty);
        let is_imm = mem::is_imm(&mut self.b, w);
        self.b.ins().brif(is_imm, imm, &[], object, &[]);
        self.b.switch_to_block(imm);
        (object, done)
    }

    /// `switch`。`int`の値とタグは`cranelift_frontend::Switch`で比べ、文字列は定数とcaseの順に比べる。定数の
    /// scrutineeは、VMと同じくコンパイルの時点で行き先を決める。`default`のない`switch`は、合うcaseがなければtrapする。
    pub(super) fn switch(&mut self, scrutinee: Atom, cases: &[Case], default: Option<BlockId>) {
        let key = |pattern: CasePattern| match pattern {
            CasePattern::Int(n) => n,
            CasePattern::Tag(tag) => i64::from(tag),
            CasePattern::String(_) => unreachable!("a string is never a constant scrutinee"),
        };
        let var = match scrutinee {
            Atom::Var(var) => var,
            Atom::Int(n) => return self.constant_switch(n, cases, default, key),
            Atom::Tag(tag) => return self.constant_switch(i64::from(tag), cases, default, key),
            _ => unreachable!("a switch reads an int, a tag or a word"),
        };
        let (otherwise, trap) = match default {
            Some(block) => (self.blocks[block.0 as usize], None),
            None => {
                let trap = self.cold_block();
                (trap, Some(trap))
            }
        };
        match cases.first().map(|case| case.pattern) {
            None => {
                self.b.ins().jump(otherwise, &[]);
            }
            Some(CasePattern::String(_)) => self.string_switch(var, cases, otherwise),
            Some(pattern) => {
                let value = match pattern {
                    CasePattern::Tag(_) => self.tag(var),
                    _ => self.var(var),
                };
                let mut switch = Switch::new();
                for case in cases {
                    // `Switch`は値のビットを比べるので、負の鍵はその2の補数である
                    switch.set_entry(
                        u128::from(key(case.pattern) as u64),
                        self.blocks[case.target.0 as usize],
                    );
                }
                switch.emit(&mut self.b, value, otherwise);
            }
        }
        if let Some(trap) = trap {
            // 合うcaseもdefaultもない。検証したプログラムはここに来ない
            self.b.switch_to_block(trap);
            self.b.ins().trap(TrapCode::unwrap_user(1));
        }
    }

    /// `var`のタグ (`I32`)。`enum`は値そのもの、`obj`はヘッダのタグ、`tobj`は即値ならその値で、物体ならヘッダの
    /// タグである。タグは16ビットに収まるので`I32`で比べ、`Switch`が`I64`の値に足す、32ビットに収まるかの比較と分岐を
    /// 省く。範囲の外の値は、飛び先の表の`default`へ進む。
    fn tag(&mut self, var: VarId) -> Value {
        let w = self.var(var);
        match self.function.repr(var) {
            Repr::Enum => self.b.ins().ireduce(types::I32, w),
            Repr::Obj => mem::load_tag(&mut self.b, w),
            Repr::TObj => {
                let (object, done) = self.on_immediate(w, types::I32);
                let small = mem::imm_value(&mut self.b, w);
                let small = self.b.ins().ireduce(types::I32, small);
                self.b.ins().jump(done, &[BlockArg::Value(small)]);
                self.b.switch_to_block(object);
                let tag = mem::load_tag(&mut self.b, w);
                self.b.ins().jump(done, &[BlockArg::Value(tag)]);
                self.b.switch_to_block(done);
                self.b.block_params(done)[0]
            }
            repr => unreachable!("a switch on tags reads `{repr:?}`"),
        }
    }

    /// 文字列の`switch`。caseの順に、文字列を`eml_x_StrEq`で定数と比べる。どちらも借りて読む。
    fn string_switch(&mut self, var: VarId, cases: &[Case], otherwise: Block) {
        let s = self.var(var);
        for case in cases {
            let CasePattern::String(index) = case.pattern else {
                unreachable!("one switch compares one kind of case")
            };
            let text = self.callees.program.strings[index as usize].clone();
            let constant = self.datum_word(Datum::String(text), types::I64);
            let equal = self
                .call_entry("eml_x_StrEq", &[s, constant])
                .expect("`eml_x_StrEq` returns an enum");
            let next = self.b.create_block();
            self.b
                .ins()
                .brif(equal, self.blocks[case.target.0 as usize], &[], next, &[]);
            self.b.switch_to_block(next);
        }
        self.b.ins().jump(otherwise, &[]);
    }

    fn constant_switch(
        &mut self,
        value: i64,
        cases: &[Case],
        default: Option<BlockId>,
        key: impl Fn(CasePattern) -> i64,
    ) {
        let target = cases
            .iter()
            .find(|case| key(case.pattern) == value)
            .map(|case| case.target)
            .or(default)
            .expect("a constant switch has a matching case or a default");
        self.b.ins().jump(self.blocks[target.0 as usize], &[]);
    }
}
