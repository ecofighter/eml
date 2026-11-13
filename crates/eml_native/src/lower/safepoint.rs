//! 入口の安全点 (docs/spec/native.md の「安全点」)。
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{Block, InstBuilder, types};
use cranelift_module::Module;
use eml_nrt::image::ACTION_CONTINUE;

use super::function::FnLower;
use super::unwind::Point;
use crate::mem;

impl<M: Module> FnLower<'_, '_, M> {
    /// 入口のブロックに安全点を出し、関数の本体の最初のブロックを返す。速い道は止める語と`stack_limit`を読み、どちらも
    /// 何も求めなければ本体へ進む。冷たい側は`eml_nrt_safepoint`で、続けさせなければ入口で持つ引数を手放して戻る。
    pub(super) fn entry_safepoint(&mut self) -> Block {
        let slow = self.cold_block();
        let leave = self.cold_block();
        let body = self.b.create_block();

        let bits = mem::stop_bits(&mut self.b, self.ctx);
        let zero = self.b.ins().iconst(types::I32, 0);
        let stopping = self.b.ins().icmp(IntCC::NotEqual, bits, zero);
        let sp = self.b.ins().get_stack_pointer(types::I64);
        let limit = mem::stack_limit(&mut self.b, self.ctx);
        let deep = self.b.ins().icmp(IntCC::UnsignedLessThan, sp, limit);
        let asked = self.b.ins().bor(stopping, deep);
        self.b.ins().brif(asked, slow, &[], body, &[]);

        self.b.switch_to_block(slow);
        let me = self.func_ref(self.id);
        let me = self.b.ins().func_addr(types::I64, me);
        let helper = eml_lir::ep_helper(&self.function.name);
        let helper = self.b.ins().iconst(types::I64, i64::from(helper));
        let action = self
            .call_entry("eml_nrt_safepoint", &[me, helper])
            .expect("the safepoint service returns an action");
        let proceed = self.b.ins().iconst(types::I64, ACTION_CONTINUE);
        let go_on = self.b.ins().icmp(IntCC::Equal, action, proceed);
        self.b.ins().brif(go_on, body, &[], leave, &[]);

        let unwinds = self.unwinds;
        let point = Point {
            owned: &unwinds.entry,
            tokens: &[],
            landing: None,
        };
        self.fill_cleanup(leave, &point);
        body
    }
}
