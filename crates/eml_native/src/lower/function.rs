//! LIRの関数1つをCLIFにする。ブロック、ブロックの引数、終端、atomである (docs/spec/native.md の「関数の形」)。
use cranelift_codegen::ir::{Block, BlockArg, InstBuilder, TrapCode, Value, types};
use cranelift_codegen::isa::TargetFrontendConfig;
use cranelift_frontend::{FunctionBuilder, Switch};
use eml_lir::{Atom, BlockId, Case, CasePattern, LirFn, Repr, Stmt, Term, VarId};

use super::kind;
use crate::abi::clif;
use crate::mem;

pub(super) fn lower(b: FunctionBuilder, function: &LirFn, config: TargetFrontendConfig) {
    let mut fx = FnLower::new(b, function);
    for (index, lir) in function.blocks.iter().enumerate() {
        fx.b.switch_to_block(fx.blocks[index]);
        for stmt in &lir.stmts {
            fx.stmt(stmt);
        }
        fx.term(&lir.term);
    }
    // 値をfrontendの変数でなくブロックの引数に置くので、すべてのブロックを下ろした後で1回sealすれば足りる
    fx.b.seal_all_blocks();
    fx.b.finalize(config);
}

pub(super) struct FnLower<'a, 'b> {
    pub b: FunctionBuilder<'b>,
    pub function: &'a LirFn,
    /// `VarId`ごとのCLIFの値。LIRは変数を1回だけ束縛する。
    vars: Vec<Option<Value>>,
    blocks: Vec<Block>,
}

impl<'a, 'b> FnLower<'a, 'b> {
    fn new(mut b: FunctionBuilder<'b>, function: &'a LirFn) -> Self {
        let blocks: Vec<Block> = function.blocks.iter().map(|_| b.create_block()).collect();
        let mut vars = vec![None; function.vars.len()];
        let entry = blocks[BlockId::ENTRY.0 as usize];
        b.append_block_params_for_function_params(entry);
        let params = b.block_params(entry).to_vec();
        for (&var, &value) in function.params().iter().zip(&params[1..]) {
            vars[var.0 as usize] = Some(value);
        }
        for (lir, &block) in function.blocks.iter().zip(&blocks).skip(1) {
            for &var in &lir.params {
                let value = b.append_block_param(block, clif(kind(function.repr(var))));
                vars[var.0 as usize] = Some(value);
            }
        }
        FnLower {
            b,
            function,
            vars,
            blocks,
        }
    }

    fn var(&self, var: VarId) -> Value {
        self.vars[var.0 as usize].unwrap_or_else(|| {
            panic!(
                "`{}` uses an unbound variable",
                self.function.vars[var.0 as usize].name
            )
        })
    }

    /// `repr`の位置での`atom`の値。定数は使う所ごとに作る。
    pub fn atom(&mut self, atom: Atom, repr: Repr) -> Value {
        match atom {
            Atom::Var(var) => self.var(var),
            Atom::Int(n) => self.b.ins().iconst(types::I64, n),
            Atom::Float(bits) => self.b.ins().f64const(bits.value()),
            Atom::Unit => self.b.ins().iconst(types::I64, mem::UNIT),
            Atom::Tag(n) => match repr {
                Repr::Enum => self.b.ins().iconst(types::I64, i64::from(n)),
                Repr::TObj => self.b.ins().iconst(types::I64, mem::imm_bits(i64::from(n))),
                _ => unreachable!("a tag sits in an `enum` or a `tobj` position"),
            },
            Atom::Fn(_) => unreachable!("function values are rejected"),
        }
    }

    fn atoms(&mut self, atoms: &[Atom], reprs: impl IntoIterator<Item = Repr>) -> Vec<Value> {
        atoms
            .iter()
            .zip(reprs)
            .map(|(&atom, repr)| self.atom(atom, repr))
            .collect()
    }

    fn stmt(&mut self, stmt: &Stmt) {
        panic!("the native lowering has no rule for `{stmt:?}`")
    }

    fn term(&mut self, term: &Term) {
        let function = self.function;
        match term {
            Term::Return(atom) => {
                let value = self.atom(*atom, function.rets[0].0);
                self.b.ins().return_(&[value]);
            }
            Term::ReturnMany(atoms) => {
                let values = self.atoms(atoms, function.rets.iter().map(|&(repr, _)| repr));
                self.b.ins().return_(&values);
            }
            Term::Jump { target, args } => {
                let params = &function.block(*target).params;
                let values = self.atoms(args, params.iter().map(|&var| function.repr(var)));
                let args: Vec<BlockArg> = values.into_iter().map(BlockArg::Value).collect();
                self.b.ins().jump(self.blocks[target.0 as usize], &args);
            }
            Term::Switch {
                scrutinee,
                layout: _,
                cases,
                default,
            } => self.switch(*scrutinee, cases, *default),
            Term::TailCall(_) => panic!("the native lowering has no rule for `{term:?}`"),
        }
    }

    /// `int`か`enum`の`switch`。定数のscrutineeは、VMと同じくコンパイルの時点で行き先を決める。
    fn switch(&mut self, scrutinee: Atom, cases: &[Case], default: Option<BlockId>) {
        let key = |pattern: CasePattern| match pattern {
            CasePattern::Int(n) => n,
            CasePattern::Tag(tag) => i64::from(tag),
            CasePattern::String(_) => panic!("the native lowering has no rule for a string switch"),
        };
        let value = match scrutinee {
            Atom::Var(var) => {
                let repr = self.function.repr(var);
                assert!(
                    matches!(repr, Repr::Int | Repr::Enum),
                    "the native lowering has no rule for a switch on `{repr:?}`"
                );
                self.var(var)
            }
            Atom::Int(n) => return self.constant_switch(n, cases, default, key),
            Atom::Tag(tag) => return self.constant_switch(i64::from(tag), cases, default, key),
            _ => unreachable!("a switch reads an int, a tag or a word"),
        };
        let trap = default.is_none().then(|| self.b.create_block());
        let otherwise = match default {
            Some(block) => self.blocks[block.0 as usize],
            None => trap.expect("a switch without a default has a trap block"),
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
        if let Some(trap) = trap {
            // 合うcaseもdefaultもない。検証したプログラムはここに来ない
            self.b.set_cold_block(trap);
            self.b.switch_to_block(trap);
            self.b.ins().trap(TrapCode::unwrap_user(1));
        }
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
