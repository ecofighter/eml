//! インラインの`dup`と`decref`。局所の数は速い道 (ロード1つ、範囲の確かめ1つ、ストア1つ) を通る。0の数 (不死の物体) は
//! 何もせず、ほかの数は遅い道のエントリを呼ぶ。即値を確かめるのは`tobj`の語だけである。`obj`の語はいつもポインタである
//! (docs/spec/native.md の「RC」、docs/spec/object-model.md の「局所の速い道」)。
use cranelift_codegen::ir::{InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;
use eml_lir::Repr;

use super::function::FnLower;
use crate::mem;

impl<M: Module> FnLower<'_, '_, M> {
    /// Repr`repr`の語`w`の参照を1つ増やす。
    pub(super) fn dup(&mut self, w: Value, repr: Repr) {
        self.counted(w, repr, mem::dup_is_local, 1, "eml_rt_dup_slow");
    }

    /// Repr`repr`の語`w`の参照を1つ手放す。
    pub(super) fn decref(&mut self, w: Value, repr: Repr) {
        self.counted(w, repr, mem::decref_is_local, -1, "eml_rt_decref");
    }

    /// `local`が真なら数に`step`を足して書き、偽なら、0の数では何もせず、ほかの数では`slow`を呼ぶ。0の確かめは0との比べで
    /// 数の定数を要さず、定数と不死の物体は呼び出しを払わない。
    fn counted(
        &mut self,
        w: Value,
        repr: Repr,
        local: fn(&mut FunctionBuilder, Value) -> Value,
        step: i64,
        slow: &str,
    ) {
        let fast = self.b.create_block();
        let other = self.b.create_block();
        let call = self.cold_block();
        let done = self.b.create_block();
        match repr {
            Repr::TObj => {
                let object = self.b.create_block();
                let imm = mem::is_imm(&mut self.b, w);
                self.b.ins().brif(imm, done, &[], object, &[]);
                self.b.switch_to_block(object);
            }
            Repr::Obj => {}
            _ => unreachable!("only `obj` and `tobj` words are counted"),
        }
        let rc = mem::load_rc(&mut self.b, w);
        let is_local = local(&mut self.b, rc);
        self.b.ins().brif(is_local, fast, &[], other, &[]);
        self.b.switch_to_block(fast);
        let next = self.b.ins().iadd_imm_s(rc, step);
        mem::store_rc(&mut self.b, w, next);
        self.b.ins().jump(done, &[]);
        self.b.switch_to_block(other);
        self.b.ins().brif(rc, call, &[], done, &[]);
        self.b.switch_to_block(call);
        self.call_entry(slow, &[w]);
        self.b.ins().jump(done, &[]);
        self.b.switch_to_block(done);
    }
}
