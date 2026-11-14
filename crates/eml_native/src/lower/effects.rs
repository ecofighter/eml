//! エフェクトの基本操作の下ろし (docs/spec/native.mdの「エフェクト」)。`yielding`と`yield_status`はyieldの状態を
//! インラインで読み、ほかの基本操作はどれもエントリ`eml_rt_<Prim::name()>`の呼び出し1つで、基本操作の即値を`Ctx`の
//! メソッドの引数の位置に置く。
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{InstBuilder, Value, types};
use cranelift_module::Module;
use eml_lir::{Atom, BlockId, Prim, VarId};

use super::consts::Datum;
use super::function::FnLower;
use super::unwind::{After, Point};
use crate::mem;

/// `prim`のエントリの記号。下ろしがエフェクトのエントリの名前を綴るのは、ここだけである。
pub(super) fn entry(prim: &Prim) -> String {
    format!("eml_rt_{}", prim.name())
}

impl<M: Module> FnLower<'_, '_, M> {
    /// `var = prim(args)`。`prim`は`empty_mut_array`でも`child_abort`でもない。
    pub(super) fn prim(
        &mut self,
        block: BlockId,
        position: usize,
        var: VarId,
        prim: &Prim,
        args: &[Atom],
    ) {
        let args = self.atoms(args, prim.row().params.iter().copied());
        let value = match prim {
            Prim::Yielding => {
                let target = mem::yield_target(&mut self.b, self.ctx);
                let yielding = self.b.ins().icmp_imm_u(IntCC::NotEqual, target, mem::UNIT);
                self.b.ins().uextend(types::I64, yielding)
            }
            Prim::YieldStatus => self.yield_status(args[0]),
            _ => {
                let (at, immediates) = self.immediates(prim);
                let mut all = args;
                all.splice(at..at, immediates);
                let result = self.call_entry(&entry(prim), &all);
                result.unwrap_or_else(|| self.b.ins().iconst(types::I64, mem::UNIT))
            }
        };
        self.bind(var, value);
        if *prim == Prim::YieldAbort {
            let point = self.point(block, position);
            self.after_unwind_point(&Point::from(point), After::Never { raise: None });
        }
    }

    /// `Ctx::yield_status`と同じ結果を、分岐でなく`select`で選ぶ。
    fn yield_status(&mut self, ev: Value) -> Value {
        let target = mem::yield_target(&mut self.b, self.ctx);
        let aborting = mem::yield_abort(&mut self.b, self.ctx);
        let constant =
            |b: &mut cranelift_frontend::FunctionBuilder, n: i64| b.ins().iconst(types::I64, n);
        let (none, to_ev, aborting_ev, other) = (
            constant(&mut self.b, 0),
            constant(&mut self.b, 1),
            constant(&mut self.b, 2),
            constant(&mut self.b, 3),
        );
        let at_ev = self.b.ins().select(aborting, aborting_ev, to_ev);
        let is_ev = self.b.ins().icmp(IntCC::Equal, target, ev);
        let yielding = self.b.ins().select(is_ev, at_ev, other);
        let idle = self.b.ins().icmp_imm_u(IntCC::Equal, target, mem::UNIT);
        self.b.ins().select(idle, none, yielding)
    }

    /// `prim`のエントリが受ける即値と、それを置く基本操作の引数の中の位置。位置は`Ctx`のメソッドの引数の位置で、
    /// 操作の添字だけがevidenceの後に来る。エフェクトの番号は`e:`の記号から、マスクの項目は`m:`の記号のアドレスとして
    /// 渡し、コードはエフェクトの番号を即値として持たない (docs/spec/native.mdの「エフェクト」)。
    fn immediates(&mut self, prim: &Prim) -> (usize, Vec<Value>) {
        let constant = |fx: &mut Self, n: usize| {
            let n = i64::try_from(n).expect("an immediate fits in 63 bits");
            fx.b.ins().iconst(types::I64, n)
        };
        match prim {
            Prim::EvvMask(items) => {
                // 項目のないマスクは0バイトのデータの物になる。LIRのverifierはdebugのビルドでそれを断り、この確かめは
                // どのビルドでも成り立つ
                assert!(
                    !items.is_empty(),
                    "`{}` masks no evidence",
                    self.function.name
                );
                let address = self.datum_address(Datum::Mask(items.clone()));
                (0, vec![address, constant(self, items.len())])
            }
            &Prim::EvNew(effect) => {
                let operations = self.callees.program.effects[effect as usize]
                    .operations
                    .len();
                let number = self.datum_word(Datum::Effect(effect), types::I64);
                (0, vec![number, constant(self, operations)])
            }
            &Prim::EvLookup(effect) => {
                (0, vec![self.datum_word(Datum::Effect(effect), types::I64)])
            }
            &Prim::EvSetClause { op, .. } | &Prim::EvClause { op, .. } => {
                (1, vec![constant(self, op as usize)])
            }
            &Prim::RootGet(root) | &Prim::RootSet(root) => (0, vec![constant(self, root as usize)]),
            _ => (0, Vec::new()),
        }
    }
}
