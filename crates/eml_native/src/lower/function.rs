//! LIRの関数1つをCLIFにする。ブロック、ブロックの引数、文、終端、atomである (docs/spec/native.md の「関数の形」)。
use std::collections::HashMap;

use cranelift_codegen::ir::{Block, BlockArg, FuncRef, InstBuilder, TrapCode, Value, types};
use cranelift_frontend::{FunctionBuilder, Switch};
use cranelift_module::{FuncId, Module};
use eml_extern::ExternRef;
use eml_lir::unwind::Unwinds;
use eml_lir::{Atom, BlockId, Case, CasePattern, LirFn, Repr, Rhs, Stmt, Term, VarId};
use eml_nrt::image::SiteRow;

use super::rows::{self, Lowering};
use super::{Imports, kind};
use crate::abi::clif;
use crate::mem;

/// `function`を下ろし、その位置の行 (`FnRow.sites`) を返す。
pub(super) fn lower<M: Module>(
    b: FunctionBuilder,
    module: &mut M,
    imports: &Imports,
    id: FuncId,
    function: &LirFn,
    unwinds: &Unwinds,
) -> Vec<SiteRow> {
    let config = module.target_config();
    let mut fx = FnLower::new(b, module, imports, id, function, unwinds);
    for (index, lir) in function.blocks.iter().enumerate() {
        fx.b.switch_to_block(fx.blocks[index]);
        for (position, stmt) in lir.stmts.iter().enumerate() {
            fx.stmt(BlockId(index as u32), position, stmt);
        }
        fx.term(&lir.term);
    }
    // 値をfrontendの変数でなくブロックの引数に置くので、すべてのブロックを下ろした後で1回sealすれば足りる
    fx.b.seal_all_blocks();
    fx.b.finalize(config);
    fx.sites
}

pub(super) struct FnLower<'a, 'b, M: Module> {
    pub b: FunctionBuilder<'b>,
    pub function: &'a LirFn,
    module: &'a mut M,
    pub imports: &'a Imports,
    /// 下ろしている関数自身。
    pub id: FuncId,
    pub unwinds: &'a Unwinds,
    /// `ctx`の語。関数の最初の引数である。
    pub ctx: Value,
    /// 関数の位置の行。添字が`eml$raise`に渡す`site`である。巻き戻しの地点のうち、失敗を上げるものだけが行を持つ。
    pub sites: Vec<SiteRow>,
    /// `VarId`ごとのCLIFの値。LIRは変数を1回だけ束縛する。
    vars: Vec<Option<Value>>,
    blocks: Vec<Block>,
    /// 関数ごとに1回だけ取り込む、呼ぶ関数の参照。
    refs: HashMap<FuncId, FuncRef>,
}

impl<'a, 'b, M: Module> FnLower<'a, 'b, M> {
    fn new(
        mut b: FunctionBuilder<'b>,
        module: &'a mut M,
        imports: &'a Imports,
        id: FuncId,
        function: &'a LirFn,
        unwinds: &'a Unwinds,
    ) -> Self {
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
            module,
            imports,
            id,
            unwinds,
            ctx: params[0],
            sites: Vec::new(),
            vars,
            blocks,
            refs: HashMap::new(),
        }
    }

    /// `id`の関数をこの関数から呼ぶ参照。
    pub fn func_ref(&mut self, id: FuncId) -> FuncRef {
        if let Some(&r) = self.refs.get(&id) {
            return r;
        }
        let r = self.module.declare_func_in_func(id, self.b.func);
        self.refs.insert(id, r);
        r
    }

    /// エントリ`symbol`を`args`で呼び、結果を返す。
    pub fn call_entry(&mut self, symbol: &str, args: &[Value]) -> Option<Value> {
        let callee = self.func_ref(self.imports.entry(symbol));
        let mut all = Vec::with_capacity(args.len() + 1);
        all.push(self.ctx);
        all.extend_from_slice(args);
        let call = self.b.ins().call(callee, &all);
        self.b.inst_results(call).first().copied()
    }

    pub fn bind(&mut self, var: VarId, value: Value) {
        self.vars[var.0 as usize] = Some(value);
    }

    pub fn var(&self, var: VarId) -> Value {
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

    fn stmt(&mut self, block: BlockId, position: usize, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                var,
                rhs:
                    Rhs::Extern {
                        ext: ExternRef::Std(ext),
                        args,
                        at,
                    },
            } if rows::lowering(*ext) == Some(Lowering::Called) => {
                self.called_row(block, position, *var, *ext, args, *at);
            }
            _ => panic!("the native lowering has no rule for `{stmt:?}`"),
        }
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
