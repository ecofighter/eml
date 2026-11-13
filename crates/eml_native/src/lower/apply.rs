//! 関数の値の適用。適用する所、クロージャのエントリ`c<k>:<f>`、`apply`の補助の関数`eml$apply$<m>`である
//! (docs/spec/native.md の「関数の値」)。
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{Block, InstBuilder, Signature, TrapCode, Value, types};
use cranelift_frontend::{FunctionBuilder, Switch};
use cranelift_module::Module;
use eml_lir::{Atom, BlockId, Call, FnIdx, LirFn, Program, Repr, Rhs, Stmt, Term, VarId};
use eml_nrt::Kind;

use super::function::FnLower;
use super::unwind::{After, Point};
use super::{Callees, Imports};
use crate::abi::tail_signature;
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

/// 値として使う関数の引数の数。関数の値として適用できるので、引数と結果の葉はどれも一様である
/// (docs/spec/lir.md の「関数の ABI」)。そうでなければ、シグネチャの食い違いでなく、ここでpanicする。
pub(super) fn arity(function: &LirFn) -> usize {
    assert!(
        function.clause.is_none(),
        "`{}` is a clause function used as a value",
        function.name
    );
    let params = function.params();
    assert!(
        !params.is_empty()
            && params.iter().all(|&var| is_uniform(function.repr(var)))
            && matches!(&function.rets[..], [(repr, _)] if is_uniform(*repr)),
        "`{}` is used as a value but its ABI is not uniform",
        function.name
    );
    params.len()
}

/// 静的なクロージャのエントリ`c0:<f>`の本体。クロージャは不死で捕まえた値を持たないので、`f`へ飛ぶだけである。
pub(super) fn define_adapter<M: Module>(
    mut b: FunctionBuilder,
    module: &mut M,
    callees: &Callees,
    function: FnIdx,
) {
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    let params = b.block_params(entry).to_vec();
    let mut args = vec![params[0]];
    args.extend_from_slice(&params[2..]);
    let callee = module.declare_func_in_func(callees.id(function), b.func);
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
