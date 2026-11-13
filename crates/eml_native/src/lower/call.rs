//! 直接の呼び出し、複数の値を返す呼び出し、末尾呼び出し (docs/spec/native.md の「関数の形」)。
use cranelift_codegen::ir::{FuncRef, InstBuilder, Value};
use cranelift_module::Module;
use eml_lir::{BlockId, Call, VarId};

use super::function::FnLower;
use super::unwind::{After, Point};

impl<M: Module> FnLower<'_, '_, M> {
    /// 末尾でない呼び出し`vars = call`。結果の葉を順に`vars`に束縛し、呼ばれた側が中断を始めていないかを確かめる
    /// (docs/spec/native.md の「巻き戻しの地点」)。
    pub(super) fn call(&mut self, block: BlockId, position: usize, vars: &[VarId], call: &Call) {
        let (callee, args) = self.direct(call);
        let inst = self.b.ins().call(callee, &args);
        let results = self.b.inst_results(inst).to_vec();
        assert_eq!(
            results.len(),
            vars.len(),
            "`{}` binds as many values as the callee returns",
            self.function.name
        );
        for (&var, value) in vars.iter().zip(results) {
            self.bind(var, value);
        }
        let point = self.point(block, position);
        self.after_unwind_point(&Point::from(point), After::Call);
    }

    /// 末尾呼び出し。呼ぶ側のフレームは呼ぶ先に置き換わるので、呼んだ後に確かめることはない。呼ぶ先が戻ったときに、
    /// 呼ぶ側の呼び手が中断を確かめる。
    pub(super) fn tail_call(&mut self, call: &Call) {
        let (callee, args) = self.direct(call);
        self.b.ins().return_call(callee, &args);
    }

    /// 直接の呼び出しの呼ぶ先と、`ctx`に続く引数。引数は、呼ぶ先の引数のReprで作る。
    fn direct(&mut self, call: &Call) -> (FuncRef, Vec<Value>) {
        let Call::Direct(callee, atoms) = call else {
            panic!("the native lowering has no rule for `{call:?}`")
        };
        let function = self.callees.program.function(*callee);
        let mut args = Vec::with_capacity(atoms.len() + 1);
        args.push(self.ctx);
        for (&atom, &param) in atoms.iter().zip(function.params()) {
            args.push(self.atom(atom, function.repr(param)));
        }
        let callee = self.func_ref(self.callees.id(*callee));
        (callee, args)
    }
}
