//! インラインの`dup`と`decref`。局所の数は速い道 (ロード1つ、範囲の確かめ1つ、ストア1つ) を通る。0の数 (不死の物体) は
//! 何もせず、ほかの数は遅い道のエントリを呼ぶ。即値を確かめるのは`tobj`の語だけである。`obj`の語はいつもポインタである
//! (docs/spec/native.md の「RC」、docs/spec/object-model.md の「局所の速い道」)。
use cranelift_codegen::ir::{FuncRef, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;
use eml_lir::Repr;

use super::function::FnLower;
use crate::mem;

/// インラインの数の操作の向き。
#[derive(Clone, Copy)]
pub(super) enum Step {
    Dup,
    Decref,
}

impl Step {
    /// 速い道で済まない数のときに呼ぶエントリ。
    pub(super) fn slow(self) -> &'static str {
        match self {
            Step::Dup => "eml_rt_dup_slow",
            Step::Decref => "eml_rt_decref",
        }
    }
}

impl<M: Module> FnLower<'_, '_, M> {
    /// Repr`repr`の語`w`の参照を1つ増やす。
    pub(super) fn dup(&mut self, w: Value, repr: Repr) {
        self.counted(w, repr, Step::Dup);
    }

    /// Repr`repr`の語`w`の参照を1つ手放す。
    pub(super) fn decref(&mut self, w: Value, repr: Repr) {
        self.counted(w, repr, Step::Decref);
    }

    fn counted(&mut self, w: Value, repr: Repr, step: Step) {
        let slow = self.func_ref(self.imports.entry(step.slow()));
        counted(&mut self.b, self.ctx, slow, w, repr, step);
    }
}

/// 語`w`の数が局所の数なら`step`の向きに1つ動かして書き、そうでなければ、0の数では何もせず、ほかの数では`slow`
/// (`step.slow()`のエントリの参照) を呼ぶ。0の確かめは0との比べで数の定数を要さず、定数と不死の物体は呼び出しを払わない。
pub(super) fn counted(
    b: &mut FunctionBuilder,
    ctx: Value,
    slow: FuncRef,
    w: Value,
    repr: Repr,
    step: Step,
) {
    let (local, delta): (fn(&mut FunctionBuilder, Value) -> Value, i64) = match step {
        Step::Dup => (mem::dup_is_local, 1),
        Step::Decref => (mem::decref_is_local, -1),
    };
    let fast = b.create_block();
    let other = b.create_block();
    let call = b.create_block();
    b.set_cold_block(call);
    let done = b.create_block();
    match repr {
        Repr::TObj => {
            let object = b.create_block();
            let imm = mem::is_imm(b, w);
            b.ins().brif(imm, done, &[], object, &[]);
            b.switch_to_block(object);
        }
        Repr::Obj => {}
        _ => unreachable!("only `obj` and `tobj` words are counted"),
    }
    let rc = mem::load_rc(b, w);
    let is_local = local(b, rc);
    b.ins().brif(is_local, fast, &[], other, &[]);
    b.switch_to_block(fast);
    let next = b.ins().iadd_imm_s(rc, delta);
    mem::store_rc(b, w, next);
    b.ins().jump(done, &[]);
    b.switch_to_block(other);
    b.ins().brif(rc, call, &[], done, &[]);
    b.switch_to_block(call);
    b.ins().call(slow, &[ctx, w]);
    b.ins().jump(done, &[]);
    b.switch_to_block(done);
}
