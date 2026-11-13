//! LIRの関数1つをCLIFにする。ブロック、ブロックの引数、文、終端、atomである (docs/spec/native.md の「関数の形」)。
use std::collections::HashMap;

use cranelift_codegen::ir::{Block, BlockArg, FuncRef, GlobalValue, InstBuilder, Value, types};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{DataId, FuncId, Module};
use eml_extern::ExternRef;
use eml_lir::unwind::Unwinds;
use eml_lir::{Atom, BlockId, Call, LirFn, Prim, Repr, Rhs, Stmt, Term, VarId};
use eml_nrt::image::SiteRow;

use super::apply::ClosureEntries;
use super::consts::{Data, Datum};
use super::data::CaseFields;
use super::rows::{self, Lowering};
use super::{Callees, Imports, kind};
use crate::abi::clif;
use crate::mem;

/// `function`を下ろし、その位置の行 (`FnRow.sites`) を返す。
#[allow(clippy::too_many_arguments)]
pub(super) fn lower<M: Module>(
    b: FunctionBuilder,
    module: &mut M,
    data: &mut Data,
    closures: &mut ClosureEntries,
    imports: &Imports,
    callees: &Callees,
    id: FuncId,
    function: &LirFn,
    unwinds: &Unwinds,
) -> Vec<SiteRow> {
    let config = module.target_config();
    let mut fx = FnLower::new(
        b, module, data, closures, imports, callees, id, function, unwinds,
    );
    for (index, lir) in function.blocks.iter().enumerate() {
        fx.b.switch_to_block(fx.blocks[index]);
        fx.case_fields(BlockId(index as u32));
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
    pub(super) module: &'a mut M,
    /// moduleの読み込み時の値の記号。関数をまたいで共有する。
    pub(super) data: &'a mut Data,
    /// moduleのクロージャのエントリ。関数をまたいで共有する。
    pub(super) closures: &'a mut ClosureEntries,
    pub imports: &'a Imports,
    pub callees: &'a Callees<'a>,
    /// 下ろしている関数自身。
    pub id: FuncId,
    pub unwinds: &'a Unwinds,
    /// `ctx`の語。関数の最初の引数である。
    pub ctx: Value,
    /// 関数の位置の行。添字が`eml$raise`に渡す`site`である。巻き戻しの地点のうち、失敗を上げるものだけが行を持つ。
    pub sites: Vec<SiteRow>,
    /// `VarId`ごとのCLIFの値。LIRは変数を1回だけ束縛する。
    vars: Vec<Option<Value>>,
    pub(super) blocks: Vec<Block>,
    /// 関数ごとに1回だけ取り込む、呼ぶ関数の参照。
    refs: HashMap<FuncId, FuncRef>,
    /// 関数ごとに1回だけ取り込む、データの記号の参照。
    pub(super) slots: HashMap<DataId, GlobalValue>,
    /// ブロックごとの、そこを行き先とする`switch`のcaseの欄。
    pub(super) cases: Vec<Option<CaseFields<'a>>>,
}

impl<'a, 'b, M: Module> FnLower<'a, 'b, M> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        mut b: FunctionBuilder<'b>,
        module: &'a mut M,
        data: &'a mut Data,
        closures: &'a mut ClosureEntries,
        imports: &'a Imports,
        callees: &'a Callees<'a>,
        id: FuncId,
        function: &'a LirFn,
        unwinds: &'a Unwinds,
    ) -> Self {
        let mut vars = vec![None; function.vars.len()];
        // CLIFの入口のブロックは安全点だけを持ち、LIRの入口のブロックへ進む。LIRの入口のブロックはどの辺の行き先でも
        // ないので、引数は入口のブロックの値のまま、どのブロックからも使える
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        let params = b.block_params(entry).to_vec();
        for (&var, &value) in function.params().iter().zip(&params[1..]) {
            vars[var.0 as usize] = Some(value);
        }
        b.switch_to_block(entry);
        let mut fx = FnLower {
            b,
            function,
            module,
            data,
            closures,
            imports,
            callees,
            id,
            unwinds,
            ctx: params[0],
            sites: Vec::new(),
            vars,
            blocks: Vec::with_capacity(function.blocks.len()),
            refs: HashMap::new(),
            slots: HashMap::new(),
            cases: CaseFields::of(function),
        };
        let body = fx.entry_safepoint();
        fx.blocks.push(body);
        for lir in function.blocks.iter().skip(1) {
            let block = fx.b.create_block();
            for &var in &lir.params {
                let value =
                    fx.b.append_block_param(block, clif(kind(function.repr(var))));
                fx.bind(var, value);
            }
            fx.blocks.push(block);
        }
        fx
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

    /// `dup`か`decref`の変数の語とRepr。verifierはそれらを数えるReprの変数にだけ許す。
    fn counted_var(&self, var: VarId) -> (Value, Repr) {
        let repr = self.function.repr(var);
        debug_assert!(
            repr.is_rc(),
            "`{}` is counted but its Repr is not",
            self.function.vars[var.0 as usize].name
        );
        (self.var(var), repr)
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
            Atom::Fn(function) => self.fn_value(function),
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
            Stmt::Let {
                var,
                rhs:
                    Rhs::Extern {
                        ext: ExternRef::Std(ext),
                        args,
                        at,
                    },
            } if let Some(Lowering::Inline(inline)) = rows::lowering(*ext) => {
                self.inline_row(block, position, *var, *ext, inline, args, *at);
            }
            Stmt::Let {
                var,
                rhs:
                    Rhs::Call(Call::Apply {
                        callee,
                        args,
                        yields: _,
                        prompt: None,
                    }),
            } => self.apply(block, position, *var, *callee, args),
            Stmt::Let {
                var,
                rhs: Rhs::Call(call),
            } => self.call(block, position, std::slice::from_ref(var), call),
            Stmt::LetMany { vars, call } => self.call(block, position, vars, call),
            Stmt::Let {
                var,
                rhs: Rhs::MakeClosure(function, args),
            } => self.make_closure(*var, *function, args),
            Stmt::Let {
                var,
                rhs: Rhs::ConstString(index),
            } => self.const_string(*var, *index),
            Stmt::Let {
                var,
                rhs: Rhs::Prim(Prim::EmptyMutArray, _),
            } => {
                let w = self.datum_word(Datum::EmptyMutArray, types::I64);
                self.bind(*var, w);
            }
            Stmt::Let {
                var,
                rhs: Rhs::Con { ctor, args, reuse },
            } => self.con(*var, *ctor, args, reuse.as_ref()),
            Stmt::Let {
                var,
                rhs: Rhs::Box(atom),
            } => self.boxed(*var, *atom),
            Stmt::Let {
                var,
                rhs: Rhs::Unbox(atom),
            } => self.unbox(*var, *atom),
            Stmt::Unpack {
                value,
                ctor,
                fields,
            } => self.unpack(*value, *ctor, fields),
            Stmt::UnboxStruct {
                value,
                ctor,
                fields,
            } => self.unbox_struct(*value, *ctor, fields),
            Stmt::Release {
                value,
                ctor,
                fields,
                token,
                unboxed,
            } => self.release(*value, *ctor, fields, *token, *unboxed),
            Stmt::DropToken(token) => self.drop_token(*token),
            Stmt::Dup(var) => {
                let (w, repr) = self.counted_var(*var);
                self.dup(w, repr);
            }
            Stmt::Decref(var) => {
                let (w, repr) = self.counted_var(*var);
                self.decref(w, repr);
            }
            Stmt::Let {
                var,
                rhs: Rhs::Drop(atom),
            } => {
                // 数える変数なら参照を1つ手放す。ほかの値は持つものがない。結果は`()`である
                if let Atom::Var(dropped) = *atom
                    && self.function.repr(dropped).is_rc()
                {
                    let w = self.var(dropped);
                    self.decref(w, self.function.repr(dropped));
                }
                let unit = self.b.ins().iconst(types::I64, mem::UNIT);
                self.bind(*var, unit);
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
            Term::TailCall(Call::Apply {
                callee,
                args,
                yields: _,
                prompt: None,
            }) => self.tail_apply(*callee, args),
            Term::TailCall(call) => self.tail_call(call),
        }
    }
}
