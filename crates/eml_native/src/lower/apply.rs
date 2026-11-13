//! 関数の値の適用。適用する所、クロージャを作る所、クロージャのエントリ`c<k>:<f>`、`apply`の補助の関数
//! `eml$apply$<m>`である (docs/spec/native.md の「関数の値」)。
use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{Block, InstBuilder, Signature, TrapCode, Value, types};
use cranelift_frontend::{FunctionBuilder, Switch};
use cranelift_module::{FuncId, Linkage, Module};
use eml_lir::{
    Atom, BlockId, Call, FnIdx, LayoutCtor, LirFn, Mark, Program, Repr, Rhs, Stmt, Term, VarId,
};
use eml_nrt::Kind;

use super::function::FnLower;
use super::rc::{self, Step};
use super::unwind::{After, Point};
use super::{Callees, Imports, kind, names};
use crate::abi::{clif, tail_signature};
use crate::mem;

/// 引数`args`個の一様な関数の値のシグネチャ`tail (ctx, clo, args x I64) -> I64`。関数の値への呼び出しとエントリは、
/// どれもこのシグネチャを持つ。
pub(super) fn uniform(args: usize) -> Signature {
    tail_signature(&vec![Kind::I64; 1 + args], &[Kind::I64])
}

/// プログラムの`apply`の引数の数の最大と1の大きい方。`eml$apply$1`は、プログラムに`apply`がなくても定義する
/// (docs/spec/native.md の「関数の値」)。
pub(super) fn largest_apply(program: &Program) -> usize {
    let mut largest = 1;
    let mut note = |call: &Call| {
        if let Call::Apply { args, .. } = call {
            largest = largest.max(args.len());
        }
    };
    for function in &program.functions {
        for block in &function.blocks {
            for stmt in &block.stmts {
                match stmt {
                    Stmt::Let {
                        rhs: Rhs::Call(call),
                        ..
                    }
                    | Stmt::LetMany { call, .. } => note(call),
                    _ => {}
                }
            }
            if let Term::TailCall(call) = &block.term {
                note(call);
            }
        }
    }
    largest
}

/// 一様な関数の値として適用できるReprか。
fn is_uniform(repr: Repr) -> bool {
    matches!(repr, Repr::Obj | Repr::TObj | Repr::Unit)
}

/// 捕まえた値を`captures`個持つ関数`function`の値が、まだ待つ引数の数。値として使う関数は、捕まえた値より後ろの引数と
/// 結果の葉がどれも一様である (docs/spec/lir.md の「関数の ABI」)。そうでなければ、シグネチャの食い違いでなく、ここでpanicする。
pub(super) fn arity(function: &LirFn, captures: usize) -> usize {
    assert!(
        function.clause.is_none(),
        "`{}` is a clause function used as a value",
        function.name
    );
    let params = function.params();
    assert!(
        captures < params.len()
            && params[captures..]
                .iter()
                .all(|&var| is_uniform(function.repr(var)))
            && matches!(&function.rets[..], [(repr, _)] if is_uniform(*repr)),
        "`{}` is used as a value with {captures} captures but its ABI is not uniform",
        function.name
    );
    params.len() - captures
}

/// 捕まえた値を`captures`個持つ関数`function`の値の欄。0番の`tobj`がコードの語で、その後に捕まえた値が関数の引数のReprで
/// 続く (docs/spec/native.md の「関数の値」)。
fn closure_fields(function: &LirFn, captures: usize) -> LayoutCtor {
    let fields = std::iter::once(Repr::TObj)
        .chain(
            function.params()[..captures]
                .iter()
                .map(|&var| function.repr(var)),
        )
        .collect();
    LayoutCtor {
        name: function.name.clone(),
        fields,
    }
}

/// 関数の値のエントリ`c<captures>:<function>`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClosureEntry {
    pub function: FnIdx,
    pub captures: u32,
}

/// moduleが宣言したクロージャのエントリ。データの記号と同じく、初めて使うときに宣言する。どのエントリも、それを名指す
/// コードが決まってから定義すれば足りるので、本体をすべて下ろした後に定義する (docs/spec/native.md の「関数の形」)。
#[derive(Default)]
pub(super) struct ClosureEntries {
    ids: HashMap<ClosureEntry, FuncId>,
    /// 最初に使った順である。
    declared: Vec<(ClosureEntry, FuncId)>,
}

impl ClosureEntries {
    /// `closure`のエントリ。初めて使うときに宣言する。
    pub(super) fn id<M: Module>(
        &mut self,
        module: &mut M,
        program: &Program,
        closure: ClosureEntry,
    ) -> FuncId {
        if let Some(&id) = self.ids.get(&closure) {
            return id;
        }
        let function = program.function(closure.function);
        let args = arity(function, closure.captures as usize);
        // 宣言は領域を使わず、名前は関数の名前と捕まえた値の数で決まり、ほかの種類の記号の接頭辞とも重ならないので、
        // 宣言が失敗するのは下ろしの誤りだけである
        let id = module
            .declare_function(
                &names::closure_entry(closure.captures, &function.name),
                Linkage::Local,
                &uniform(args),
            )
            .unwrap_or_else(|error| panic!("a closure entry cannot be declared: {error}"));
        self.ids.insert(closure, id);
        self.declared.push((closure, id));
        id
    }

    /// 宣言したエントリ。最初に使った順である。
    pub(super) fn declared(self) -> Vec<(ClosureEntry, FuncId)> {
        self.declared
    }
}

/// クロージャのエントリ`c<k>:<f>`の本体。捕まえた値をReprごとにロードし、`f`を`(ctx, 捕まえた値.., 引数..)`で末尾で
/// 呼ぶ。捕まえた値の参照は`f`へ移る。クロージャが最後の参照なら、その参照を受け継いだまま箱だけを空け、共有なら捕まえた
/// 数える値を`dup`してからクロージャの参照を手放す。手放す前に`dup`するので、ほかの持ち主が同時に最後の参照を手放しても、
/// 捕まえた値は生きている。静的なクロージャ (`k = 0`) は不死で捕まえた値を持たないので、`f`へ飛ぶだけである
/// (docs/spec/native.md の「関数の値」)。
pub(super) fn define_closure_entry<M: Module>(
    mut b: FunctionBuilder,
    module: &mut M,
    imports: &Imports,
    callees: &Callees,
    closure: ClosureEntry,
) {
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    let params = b.block_params(entry).to_vec();
    let (ctx, clo) = (params[0], params[1]);
    let function = callees.program.function(closure.function);
    let captures = closure.captures as usize;
    let mut args = vec![ctx];
    if captures > 0 {
        let fields = closure_fields(function, captures);
        let shape = fields.shape();
        let loaded: Vec<(Value, Repr)> = fields.fields[1..]
            .iter()
            .zip(&shape.positions[1..])
            .map(|(&repr, &position)| {
                let ty = clif(kind(repr));
                (
                    mem::load_field(&mut b, clo, shape.scanned, position, ty),
                    repr,
                )
            })
            .collect();
        let mut entry_ref = |b: &mut FunctionBuilder, symbol: &str| {
            module.declare_func_in_func(imports.entry(symbol), b.func)
        };
        let is_last = entry_ref(&mut b, "eml_rt_is_last");
        let free_block = entry_ref(&mut b, "eml_rt_free_block");
        let dup = entry_ref(&mut b, Step::Dup.slow());
        let decref = entry_ref(&mut b, Step::Decref.slow());
        let last = b.create_block();
        let ask = b.create_block();
        b.set_cold_block(ask);
        let shared = b.create_block();
        let go = b.create_block();
        let rc = mem::load_rc(&mut b, clo);
        let unique = mem::is_one_ref(&mut b, rc);
        b.ins().brif(unique, last, &[], ask, &[]);
        b.switch_to_block(ask);
        let call = b.ins().call(is_last, &[ctx, clo]);
        let answer = b.inst_results(call)[0];
        b.ins().brif(answer, last, &[], shared, &[]);
        b.switch_to_block(last);
        b.ins().call(free_block, &[ctx, clo]);
        b.ins().jump(go, &[]);
        b.switch_to_block(shared);
        for &(value, repr) in &loaded {
            if repr.is_rc() {
                rc::counted(&mut b, ctx, dup, value, repr, Step::Dup);
            }
        }
        rc::counted(&mut b, ctx, decref, clo, Repr::Obj, Step::Decref);
        b.ins().jump(go, &[]);
        b.switch_to_block(go);
        args.extend(loaded.iter().map(|&(value, _)| value));
    }
    args.extend_from_slice(&params[2..]);
    let callee = module.declare_func_in_func(callees.id(closure.function), b.func);
    b.ins().return_call(callee, &args);
    b.seal_all_blocks();
    b.finalize(module.target_config());
}

/// `eml$apply$<args>`の本体。呼ぶ値`clo`が待つ引数の数`n`で分ける。`n == args`なら値のエントリへ末尾で飛ぶ。
/// `n > args`ならPAPを作って返す。`n < args`なら最初の`n`個で呼び、中断を確かめて、結果に残りを適用する。どの腕も
/// `clo`と引数の参照を、呼ぶ先かPAPへ渡すか、中断のときに手放す。
pub(super) fn define_apply<M: Module>(
    mut b: FunctionBuilder,
    module: &mut M,
    imports: &Imports,
    args: usize,
) {
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    let params = b.block_params(entry).to_vec();
    let (ctx, clo, values) = (params[0], params[1], &params[2..]);
    let meta = mem::load_meta(&mut b, clo);
    let n = mem::meta_tag(&mut b, meta);
    let exact = b.create_block();
    let inexact = b.create_block();
    let is_exact = b.ins().icmp_imm_u(IntCC::Equal, n, args as i64);
    b.ins().brif(is_exact, exact, &[], inexact, &[]);

    b.switch_to_block(exact);
    let code = mem::closure_code(&mut b, clo, meta);
    let sig = b.import_signature(uniform(args));
    b.ins().return_call_indirect(sig, code, &params);

    b.switch_to_block(inexact);
    let pap = b.create_block();
    let fewer = b.create_block();
    let waits = b
        .ins()
        .icmp_imm_u(IntCC::UnsignedGreaterThan, n, args as i64);
    b.ins().brif(waits, pap, &[], fewer, &[]);

    // `eml_nrt_pap`は`clo`と引数を受け取るので、ここでは何も手放さない
    b.switch_to_block(pap);
    let argv = mem::spill(&mut b, values);
    let count = b.ins().iconst(types::I64, args as i64);
    let service = module.declare_func_in_func(imports.entry("eml_nrt_pap"), b.func);
    let call = b.ins().call(service, &[ctx, clo, argv, count]);
    let made = b.inst_results(call)[0];
    b.ins().return_(&[made]);

    b.switch_to_block(fewer);
    let trap = b.create_block();
    b.set_cold_block(trap);
    let mut switch = Switch::new();
    let arms: Vec<_> = (1..args)
        .map(|first| {
            let arm = b.create_block();
            switch.set_entry(first as u128, arm);
            (first, arm)
        })
        .collect();
    switch.emit(&mut b, n, trap);
    for (first, arm) in arms {
        b.switch_to_block(arm);
        let code = mem::closure_code(&mut b, clo, meta);
        let sig = b.import_signature(uniform(first));
        let mut head = vec![ctx, clo];
        head.extend_from_slice(&values[..first]);
        let call = b.ins().call_indirect(sig, code, &head);
        let result = b.inst_results(call)[0];
        // 最初の部分の中で中断が始まったら、残りの引数を1回ずつ手放し、仮の値を返す。中断しうるのは1つの`apply`の
        // 最後の矢印だけだが、その矢印がどの部分に来るかは実行時に決まる (docs/spec/native.md の「関数の値」)
        let rest = &values[first..];
        let target = mem::yield_target(&mut b, ctx);
        let unit = b.ins().iconst(types::I64, mem::UNIT);
        let aborting = b.ins().icmp(IntCC::NotEqual, target, unit);
        let cleanup = b.create_block();
        b.set_cold_block(cleanup);
        let hot = b.create_block();
        b.ins().brif(aborting, cleanup, &[], hot, &[]);
        b.switch_to_block(cleanup);
        let decref = module.declare_func_in_func(imports.entry("eml_rt_decref"), b.func);
        for &value in rest {
            b.ins().call(decref, &[ctx, value]);
        }
        let placeholder = b.ins().iconst(types::I64, mem::UNIT);
        b.ins().return_(&[placeholder]);
        b.switch_to_block(hot);
        let next = module.declare_func_in_func(imports.apply(rest.len()), b.func);
        let mut tail = vec![ctx, result];
        tail.extend_from_slice(rest);
        b.ins().return_call(next, &tail);
    }
    // 関数の値のタグは1以上なので、検証したプログラムはここへ来ない
    b.switch_to_block(trap);
    b.ins().trap(TrapCode::unwrap_user(1));
    b.seal_all_blocks();
    b.finalize(module.target_config());
}

/// 適用する所の分かれ目。`exact`は呼ぶ値が引数の数だけを待つときの道で、`slow`はほかの道である。`values`は
/// `ctx`、呼ぶ値、引数の順である。
struct Dispatch {
    exact: Block,
    slow: Block,
    meta: Value,
    values: Vec<Value>,
}

impl<M: Module> FnLower<'_, '_, M> {
    /// `let var = closure function(args)`。`eml_rt_alloc_data`で物体を作り、コードの語と捕まえた値を欄に書く。タグは
    /// まだ待つ引数の数で、`args`の所有は物体へ移る (docs/spec/native.md の「関数の値」)。
    pub(super) fn make_closure(&mut self, var: VarId, function: FnIdx, args: &[Atom]) {
        let program = self.callees.program;
        let target = program.function(function);
        let fields = closure_fields(target, args.len());
        let shape = fields.shape();
        let closure = ClosureEntry {
            function,
            captures: u32::try_from(args.len()).expect("a closure has fewer than 2^32 captures"),
        };
        let id = self.closures.id(self.module, program, closure);
        let entry = self.func_ref(id);
        let address = self.b.ins().func_addr(types::I64, entry);
        let code = mem::make_imm(&mut self.b, address);
        let mut values = vec![code];
        values.extend(
            args.iter()
                .zip(&fields.fields[1..])
                .map(|(&arg, &repr)| self.atom(arg, repr)),
        );
        let lin = self.function.mark(var) == Mark::Lin;
        let head = [
            arity(target, args.len()) as i64,
            i64::from(shape.scanned),
            i64::from(shape.raw),
            i64::from(lin),
        ]
        .map(|n| self.b.ins().iconst(types::I64, n));
        let w = self.alloc_con(&head, &shape, &values);
        self.bind(var, w);
    }

    /// 末尾でない`var = apply callee(args)`。呼ぶ値が引数の数だけを待てば、そのエントリを直接呼び、そうでなければ
    /// `eml$apply$<m>`を呼ぶ。その後に中断を確かめる。
    pub(super) fn apply(
        &mut self,
        block: BlockId,
        position: usize,
        var: VarId,
        callee: Atom,
        args: &[Atom],
    ) {
        let dispatch = self.dispatch(callee, args);
        let join = self.b.create_block();
        let result = self.b.append_block_param(join, types::I64);

        self.b.switch_to_block(dispatch.exact);
        let clo = dispatch.values[1];
        let code = mem::closure_code(&mut self.b, clo, dispatch.meta);
        let sig = self.b.import_signature(uniform(args.len()));
        let call = self.b.ins().call_indirect(sig, code, &dispatch.values);
        let r = self.b.inst_results(call)[0];
        self.b.ins().jump(join, &[r.into()]);

        self.b.switch_to_block(dispatch.slow);
        let helper = self.func_ref(self.imports.apply(args.len()));
        let call = self.b.ins().call(helper, &dispatch.values);
        let r = self.b.inst_results(call)[0];
        self.b.ins().jump(join, &[r.into()]);

        self.b.switch_to_block(join);
        self.bind(var, result);
        let point = self.point(block, position);
        self.after_unwind_point(&Point::from(point), After::Call);
    }

    /// 末尾の`apply callee(args)`。`apply`と同じに分け、どちらの道も末尾で呼ぶ。
    pub(super) fn tail_apply(&mut self, callee: Atom, args: &[Atom]) {
        let dispatch = self.dispatch(callee, args);

        self.b.switch_to_block(dispatch.exact);
        let clo = dispatch.values[1];
        let code = mem::closure_code(&mut self.b, clo, dispatch.meta);
        let sig = self.b.import_signature(uniform(args.len()));
        self.b
            .ins()
            .return_call_indirect(sig, code, &dispatch.values);

        self.b.switch_to_block(dispatch.slow);
        let helper = self.func_ref(self.imports.apply(args.len()));
        self.b.ins().return_call(helper, &dispatch.values);
    }

    /// 呼ぶ値のタグを読み、引数の数と比べて分ける。
    fn dispatch(&mut self, callee: Atom, args: &[Atom]) -> Dispatch {
        let clo = self.atom(callee, Repr::TObj);
        let mut values = vec![self.ctx, clo];
        values.extend(args.iter().map(|&atom| self.atom(atom, Repr::TObj)));
        let meta = mem::load_meta(&mut self.b, clo);
        let n = mem::meta_tag(&mut self.b, meta);
        let exact = self.b.create_block();
        let slow = self.b.create_block();
        let is_exact = self.b.ins().icmp_imm_u(IntCC::Equal, n, args.len() as i64);
        self.b.ins().brif(is_exact, exact, &[], slow, &[]);
        Dispatch {
            exact,
            slow,
            meta,
            values,
        }
    }
}
