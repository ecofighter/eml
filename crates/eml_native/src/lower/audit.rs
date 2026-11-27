//! 検査モードの下ろし方の監査 (docs/spec/native.mdの「検査モード」)。検査モードの下ろし方は、速いモードの下ろし方と
//! 同じインラインの読み書きを出し、その前にその読み書きの監査のエントリの呼び出しを1つ出す。`mem::`の読み書きの補助を
//! 名指す`lower/`のmoduleは、どれもこのmoduleを通して監査を出す。速いモードの下ろし方は何も出さない。ここのどの関数も、
//! 検査モードの下ろし方でなければすぐに戻る。
use std::collections::HashMap;

use cranelift_codegen::ir::{FuncRef, InstBuilder, MemFlagsData, Value, types};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use super::Imports;
use super::function::FnLower;
use super::rc::Step;
use crate::mem;

/// 監査1つ。物体の語は横に渡す。
#[derive(Clone, Copy)]
pub(super) enum Audit {
    /// 走査する語が`scanned`個の物体の、位置`position`の欄のロードの前。
    Load { scanned: u32, position: u32 },
    /// その欄への`value`のストアの前。
    Store {
        scanned: u32,
        position: u32,
        value: Value,
    },
    /// `load_rc`と`is_one_ref`の前。
    Unique,
    /// `load_meta`、`load_tag`、`is_big_int`の前。
    Header,
    /// 関数の値の`load_meta`と`closure_code`と、そのコードの間接の呼び出しの前。`expect`は呼び出しが使う署名の語か、
    /// `CODE_OF_TAG`である。
    Code { expect: Expect },
    /// `load_box`の前。
    Boxed { float: bool },
    /// 構築子を名指す文の欄のロードの前。`what` (`DATA_*`) は、食い違ったときのVMの文言を選ぶ。
    Data { tag: u32, scanned: u32, what: i64 },
}

/// 関数の値を通した間接の呼び出しが待つ語。定数か、データの記号からロードした語である。
#[derive(Clone, Copy)]
pub(super) enum Expect {
    /// コードに書いた、呼び出しの署名の語。
    Word(i64),
    /// `sig_word`が書けない署名の語。ホストが埋めるデータの記号`w:`からロードする。
    Loaded(Value),
}

/// `step`のインラインの数の操作の前の監査のエントリ。
pub(super) fn rc_entry(step: Step) -> &'static str {
    match step {
        Step::Dup => "eml_rt_audit_dup",
        Step::Decref => "eml_rt_audit_decref",
    }
}

/// `obj`についての数の監査`audit`の呼び出しを出す。速いモードの下ろし方は`None`を渡す。
pub(super) fn call_before(b: &mut FunctionBuilder, ctx: Value, audit: Option<FuncRef>, obj: Value) {
    if let Some(audit) = audit {
        b.ins().call(audit, &[ctx, obj]);
    }
}

/// エントリと、`ctx`の後の引数。
fn call(b: &mut FunctionBuilder, obj: Value, audit: Audit) -> (&'static str, Vec<Value>) {
    let word = |b: &mut FunctionBuilder, n: i64| b.ins().iconst(types::I64, n);
    match audit {
        Audit::Load { scanned, position } => {
            let index = word(b, mem::field_index(scanned, position) as i64);
            let entry = if position < scanned {
                "eml_rt_audit_field"
            } else {
                "eml_rt_audit_raw"
            };
            (entry, vec![obj, index])
        }
        Audit::Store {
            scanned,
            position,
            value,
        } => {
            let index = word(b, mem::field_index(scanned, position) as i64);
            if position < scanned {
                ("eml_rt_audit_store", vec![obj, index, value])
            } else {
                let bits = if b.func.dfg.value_type(value) == types::F64 {
                    b.ins().bitcast(types::I64, MemFlagsData::new(), value)
                } else {
                    value
                };
                ("eml_rt_audit_store_raw", vec![obj, index, bits])
            }
        }
        Audit::Unique => ("eml_rt_audit_unique", vec![obj]),
        Audit::Header => ("eml_rt_audit_header", vec![obj]),
        Audit::Code { expect } => {
            let expect = match expect {
                Expect::Word(n) => word(b, n),
                Expect::Loaded(value) => value,
            };
            ("eml_rt_audit_code", vec![obj, expect])
        }
        Audit::Boxed { float } => {
            let float = word(b, i64::from(float));
            ("eml_rt_audit_box", vec![obj, float])
        }
        Audit::Data { tag, scanned, what } => {
            let tag = word(b, i64::from(tag));
            let scanned = word(b, i64::from(scanned));
            let what = word(b, what);
            ("eml_rt_audit_data", vec![obj, tag, scanned, what])
        }
    }
}

/// `FnLower`を持たない補助の関数1つ (クロージャのエントリ、PAPのアダプタ、`eml$apply$<m>`、`eml$raise`) の監査。
/// 下ろし方のimport、関数の`ctx`、エントリごとに1つの宣言を持つ。
pub(super) struct Glue<'a> {
    pub(super) imports: &'a Imports,
    pub(super) ctx: Value,
    refs: HashMap<&'static str, FuncRef>,
}

impl<'a> Glue<'a> {
    pub(super) fn new(imports: &'a Imports, ctx: Value) -> Glue<'a> {
        Glue {
            imports,
            ctx,
            refs: HashMap::new(),
        }
    }

    /// エントリ`symbol`の関数の参照。この補助の関数で1回だけ宣言する。
    pub(super) fn entry<M: Module>(
        &mut self,
        b: &mut FunctionBuilder,
        module: &mut M,
        symbol: &'static str,
    ) -> FuncRef {
        let id = self.imports.entry(symbol);
        *self
            .refs
            .entry(symbol)
            .or_insert_with(|| module.declare_func_in_func(id, b.func))
    }

    /// `obj`の`audit`を出す。検査モードの下ろし方でだけ出す。
    pub(super) fn emit<M: Module>(
        &mut self,
        b: &mut FunctionBuilder,
        module: &mut M,
        obj: Value,
        audit: Audit,
    ) {
        if !self.imports.checked {
            return;
        }
        let (symbol, mut args) = call(b, obj, audit);
        let callee = self.entry(b, module, symbol);
        args.insert(0, self.ctx);
        b.ins().call(callee, &args);
    }

    /// workerの数`which` (`COUNT_*`) を1つ数える呼び出しを出す。検査モードの下ろし方でだけ出す。
    pub(super) fn count<M: Module>(&mut self, b: &mut FunctionBuilder, module: &mut M, which: i64) {
        if !self.imports.checked {
            return;
        }
        let callee = self.entry(b, module, "eml_rt_audit_count");
        let which = b.ins().iconst(types::I64, which);
        b.ins().call(callee, &[self.ctx, which]);
    }

    /// `dup`と`decref`の監査 (`rc::counted`)。速いモードの下ろし方では`None`である。
    pub(super) fn rc_refs<M: Module>(
        &mut self,
        b: &mut FunctionBuilder,
        module: &mut M,
    ) -> Option<(FuncRef, FuncRef)> {
        self.imports.checked.then(|| {
            (
                self.entry(b, module, rc_entry(Step::Dup)),
                self.entry(b, module, rc_entry(Step::Decref)),
            )
        })
    }
}

impl<M: Module> FnLower<'_, '_, M> {
    /// 関数のbuilderを通して、`obj`の`audit`を出す。検査モードの下ろし方でだけ出す。
    pub(super) fn audit(&mut self, obj: Value, audit: Audit) {
        if !self.imports.checked {
            return;
        }
        let (symbol, args) = call(&mut self.b, obj, audit);
        self.call_entry(symbol, &args);
    }

    /// workerの数`which` (`COUNT_*`) を1つ数える呼び出しを出す。検査モードの下ろし方でだけ出す。
    pub(super) fn count(&mut self, which: i64) {
        if !self.imports.checked {
            return;
        }
        let which = self.b.ins().iconst(types::I64, which);
        self.call_entry("eml_rt_audit_count", &[which]);
    }
}
