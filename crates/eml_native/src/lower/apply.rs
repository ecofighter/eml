//! 関数の値の適用。適用する所、クロージャを作る所、クロージャのエントリ`c<k>:<f>`、`apply`の補助の関数
//! `eml$apply$<m>`である (docs/spec/native.md の「関数の値」)。
use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{Block, InstBuilder, Signature, TrapCode, Value, types};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{FuncId, Linkage, Module};
use eml_lir::{
    Atom, BlockId, Call, FnIdx, LayoutCtor, LirFn, Mark, OpRet, Program, Repr, Rhs, Stmt, Term,
    VarId,
};
use eml_nrt::Kind;
use eml_nrt::image::ACTION_CONTINUE;

use super::function::FnLower;
use super::rc::{self, Step};
use super::unwind::{self, After, Point};
use super::{Callees, Imports, kind, names};
use crate::abi::{clause_signature, clif, closure_entry_signature, tail_signature};
use crate::mem;

/// 引数`args`個の一様な関数の値のシグネチャ`tail (ctx, clo, args x I64) -> I64`。関数の値への呼び出しとエントリは、
/// どれもこのシグネチャを持つ。
pub(super) fn uniform(args: usize) -> Signature {
    tail_signature(&vec![Kind::I64; 1 + args], &[Kind::I64])
}

/// `meta`の語が`meta`のクロージャ`callee`のコードを、`(ctx, callee, args..)`とシグネチャ`sig`で間接に呼ぶ。末尾呼び出し
/// は何も返さず、ほかの呼び出しは結果を返す。関数の値のコードの呼び出しは、どれもここを通る。
pub(super) fn call_closure_code(
    b: &mut FunctionBuilder,
    ctx: Value,
    callee: Value,
    meta: Value,
    sig: Signature,
    args: &[Value],
    tail: bool,
) -> Vec<Value> {
    let code = mem::closure_code(b, callee, meta);
    let sig = b.import_signature(sig);
    let mut values = Vec::with_capacity(args.len() + 2);
    values.extend([ctx, callee]);
    values.extend_from_slice(args);
    if tail {
        b.ins().return_call_indirect(sig, code, &values);
        Vec::new()
    } else {
        let call = b.ins().call_indirect(sig, code, &values);
        b.inst_results(call).to_vec()
    }
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

/// 捕まえた値を`captures`個持つ関数`function`の値が、まだ待つ引数の数で、関数の値のタグである。値として使う関数は、
/// 捕まえた値より後ろの引数と結果の葉がどれも一様である (docs/spec/lir.mdの「関数の ABI」)。そうでなければ、シグネチャの
/// 食い違いでなく、ここでpanicする。タグは16ビットなので、65535を超える数でもpanicする。節の関数の値のタグは0で、
/// `apply`で呼ばれることはなく、そのエントリは操作のシグネチャを持つ (docs/spec/native.mdの「関数の値」)。
pub(super) fn arity(function: &LirFn, captures: usize) -> u16 {
    if function.clause.is_some() {
        return 0;
    }
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
    u16::try_from(params.len() - captures).unwrap_or_else(|_| {
        panic!(
            "`{}` waits for more than 65535 arguments as a value",
            function.name
        )
    })
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
        // 形の正しくない関数を値として使えば、関数の残りを下ろす前に、ここで宣言のときにpanicする
        arity(function, closure.captures as usize);
        // 宣言は領域を使わず、名前は関数の名前と捕まえた値の数で決まり、ほかの種類の記号の接頭辞とも重ならないので、
        // 宣言が失敗するのは下ろしの誤りだけである
        let id = module
            .declare_function(
                &names::closure_entry(closure.captures, &function.name),
                Linkage::Local,
                &closure_entry_signature(function, closure.captures as usize),
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
/// 呼ぶ。捕まえた値の参照は、`release_box`を通して`f`へ移る。静的なクロージャ (`k = 0`) は不死で捕まえた値を持たない
/// ので、`f`へ飛ぶだけである
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
        release_box(&mut b, module, imports, ctx, clo, &loaded);
        args.extend(loaded.iter().map(|&(value, _)| value));
    }
    args.extend_from_slice(&params[2..]);
    let callee = module.declare_func_in_func(callees.id(closure.function), b.func);
    b.ins().return_call(callee, &args);
    b.seal_all_blocks();
    b.finalize(module.target_config());
}

/// 欄を`loaded`にロードした箱`clo`の参照を手放し、欄の参照を呼ぶ先へ渡せるようにする。箱が最後の参照なら、欄の参照を
/// 受け継いだまま箱だけを空け、共有なら数える欄を`dup`してから箱の参照を手放す。手放す前に`dup`するので、ほかの持ち主が
/// 同時に最後の参照を手放しても、欄の値は生きている。
fn release_box<M: Module>(
    b: &mut FunctionBuilder,
    module: &mut M,
    imports: &Imports,
    ctx: Value,
    clo: Value,
    loaded: &[(Value, Repr)],
) {
    let mut entry_ref = |b: &mut FunctionBuilder, symbol: &str| {
        module.declare_func_in_func(imports.entry(symbol), b.func)
    };
    let is_last = entry_ref(b, "eml_rt_is_last");
    let free_block = entry_ref(b, "eml_rt_free_block");
    let dup = entry_ref(b, Step::Dup.slow());
    let decref = entry_ref(b, Step::Decref.slow());
    let last = b.create_block();
    let ask = b.create_block();
    b.set_cold_block(ask);
    let shared = b.create_block();
    let go = b.create_block();
    let rc = mem::load_rc(b, clo);
    let unique = mem::is_one_ref(b, rc);
    b.ins().brif(unique, last, &[], ask, &[]);
    b.switch_to_block(ask);
    let call = b.ins().call(is_last, &[ctx, clo]);
    let answer = b.inst_results(call)[0];
    b.ins().brif(answer, last, &[], shared, &[]);
    b.switch_to_block(last);
    b.ins().call(free_block, &[ctx, clo]);
    b.ins().jump(go, &[]);
    b.switch_to_block(shared);
    for &(value, repr) in loaded {
        if repr.is_rc() {
            rc::counted(b, ctx, dup, value, repr, Step::Dup);
        }
    }
    rc::counted(b, ctx, decref, clo, Repr::Obj, Step::Decref);
    b.ins().jump(go, &[]);
    b.switch_to_block(go);
}

/// PAPの走査する語の数。アダプタのコード、呼ぶ値、引数1つである。
const PAP_SCANNED: u32 = 3;

/// 関数の値が待つ引数の数の最大が`largest`のプログラムで、`eml_nrt_pap`が求めうるPAPのアダプタの、待つ引数の数。
/// PAPの呼ぶ値は、クロージャのエントリを持つ値か別のPAPで、PAPはその呼ぶ値より1つ少なく待つので、`largest - 1`を
/// 超えない (docs/spec/native.md の「関数の値」)。
pub(super) fn pap_rests(largest: u16) -> std::ops::Range<u32> {
    1..u32::from(largest)
}

/// PAPのアダプタ`eml$pap$<rest>`の本体。自分の欄から呼ぶ値`callee`と引数`a`をロードし、箱の参照を手放して、`callee`を
/// `(ctx, callee, a, b_1..b_rest)`で末尾で間接に呼ぶ。`callee`の形は分からないので、そのコードは`meta`の語から読む
/// (docs/spec/native.md の「関数の値」)。
pub(super) fn define_pap_adapter<M: Module>(
    mut b: FunctionBuilder,
    module: &mut M,
    imports: &Imports,
    rest: u32,
) {
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    let params = b.block_params(entry).to_vec();
    let (ctx, pap) = (params[0], params[1]);
    // `eml_nrt_pap`は即値の`callee`を断るので、`callee`はいつも物体である。引数は一様な語で、即値でありうる
    let loaded = [(1, Repr::Obj), (2, Repr::TObj)].map(|(position, repr)| {
        (
            mem::load_field(&mut b, pap, PAP_SCANNED, position, types::I64),
            repr,
        )
    });
    release_box(&mut b, module, imports, ctx, pap, &loaded);
    let callee = loaded[0].0;
    let meta = mem::load_meta(&mut b, callee);
    let mut args = vec![loaded[1].0];
    args.extend_from_slice(&params[2..]);
    let sig = uniform(rest as usize + 1);
    call_closure_code(&mut b, ctx, callee, meta, sig, &args, true);
    b.seal_all_blocks();
    b.finalize(module.target_config());
}

/// `eml$apply$<args>`の本体。呼ぶ値`clo`が待つ引数の数`n`で分ける。`n == args`なら値のエントリへ末尾で飛ぶ。
/// `n > args`ならPAPを作って返す。`n < args`なら最後の引数を除いた`args - 1`個を`eml$apply$(args-1)`で適用し、中断を
/// 確かめて、結果を最後の引数に適用する。入れ子にする前に、入口の安全点と同じくスタックの上限を確かめる。どの腕も`clo`と
/// 引数の参照を、呼ぶ先かPAPへ渡すか、中断のときに手放す。
/// `(args, n)`ごとの腕を持たないので、補助の関数のコードは`args`に比例する。
pub(super) fn define_apply<M: Module>(
    mut b: FunctionBuilder,
    module: &mut M,
    imports: &Imports,
    args: usize,
) {
    let own = imports.apply(args);
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
    call_closure_code(&mut b, ctx, clo, meta, uniform(args), values, true);

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
    if args == 1 {
        // 関数の値のタグは1以上なので、検証したプログラムはここへ来ない
        b.ins().trap(TrapCode::unwrap_user(1));
    } else {
        // 入れ子の補助の関数のフレームは、emlの関数の入口が確かめるより前に`args`の2乗で積み重なるので、入れ子にする前に
        // 入口の安全点と同じ上限を確かめる。下ならサービスが深さの上限でrunを終え、戻らない。この関数は`Image`の
        // 関数の行にないので、誤りは`<native code>`を名指す (docs/spec/native.md の「安全点」)
        let deep = mem::below_stack_limit(&mut b, ctx);
        let check = b.create_block();
        b.set_cold_block(check);
        let nest = b.create_block();
        b.ins().brif(deep, check, &[], nest, &[]);
        b.switch_to_block(check);
        let me = module.declare_func_in_func(own, b.func);
        let me = b.ins().func_addr(types::I64, me);
        let no_helper = b.ins().iconst(types::I64, 0);
        let safepoint = module.declare_func_in_func(imports.entry("eml_nrt_safepoint"), b.func);
        // サービスは`ACTION_CONTINUE`を返すか、戻らずにrunを終えるだけである。自分のスタックポインタは上限より下なので、
        // ここでは戻らない。サービスがほかの行動を返すようになったら、入口の安全点と同じく後始末へ分けなければならない
        // ので、それまではほかの行動をtrapにする
        let call = b.ins().call(safepoint, &[ctx, me, no_helper]);
        let action = b.inst_results(call)[0];
        let proceed = b.ins().icmp_imm_u(IntCC::Equal, action, ACTION_CONTINUE);
        let other = b.create_block();
        b.set_cold_block(other);
        b.ins().brif(proceed, nest, &[], other, &[]);
        b.switch_to_block(other);
        b.ins().trap(TrapCode::unwrap_user(1));
        b.switch_to_block(nest);
        // `n <= args - 1`なので、内の呼び出しはPAPを作らない
        let (head, last) = values.split_at(args - 1);
        let last = last[0];
        let inner = module.declare_func_in_func(imports.apply(args - 1), b.func);
        let mut call_args = vec![ctx, clo];
        call_args.extend_from_slice(head);
        let call = b.ins().call(inner, &call_args);
        let result = b.inst_results(call)[0];
        // 内の適用の中で中断が始まったら、最後の引数を1回手放し、仮の値を返す。中断しうるのは1つの`apply`の最後の
        // 矢印だけだが、その矢印がどの部分に来るかは実行時に決まる。残りの引数は、内の段がそれぞれ1回ずつ手放す
        // (docs/spec/native.md の「関数の値」)
        let (cleanup, hot) = unwind::if_aborting(&mut b, ctx, &[]);
        b.switch_to_block(cleanup);
        let decref = module.declare_func_in_func(imports.entry("eml_rt_decref"), b.func);
        b.ins().call(decref, &[ctx, last]);
        let placeholder = b.ins().iconst(types::I64, mem::UNIT);
        b.ins().return_(&[placeholder]);
        b.switch_to_block(hot);
        // 1つの`apply`のうちyieldしうるのは最後の矢印だけなので、最後でない部分からyieldが戻ることはない。ここで
        // `target`がUnitでなければそういうyieldなので、そのダミーを関数の値として渡す代わりにtrapする
        // (docs/spec/native.md の「関数の値」)
        let target = mem::yield_target(&mut b, ctx);
        let yielding = b.ins().icmp_imm_u(IntCC::NotEqual, target, mem::UNIT);
        let misplaced = b.create_block();
        b.set_cold_block(misplaced);
        let apply = b.create_block();
        b.ins().brif(yielding, misplaced, &[], apply, &[]);
        b.switch_to_block(misplaced);
        b.ins().trap(TrapCode::unwrap_user(1));
        b.switch_to_block(apply);
        let next = module.declare_func_in_func(imports.apply(1), b.func);
        b.ins().return_call(next, &[ctx, result, last]);
    }
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
            i64::from(arity(target, args.len())),
            i64::from(shape.scanned),
            i64::from(shape.raw),
            i64::from(lin),
        ]
        .map(|n| self.b.ins().iconst(types::I64, n));
        let w = self.alloc_con(&head, &shape, &values);
        self.bind(var, w);
    }

    /// 末尾でない`var = apply callee(args)`。呼ぶ値が引数の数だけを待てば、そのエントリを直接呼び、そうでなければ
    /// `eml$apply$<m>`を呼ぶ。その後に中断を確かめる。着地の印を持つ地点では、その確かめが、印を宛先とする中断を
    /// ここに着地させる。
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
        let r = call_closure_code(
            &mut self.b,
            self.ctx,
            dispatch.values[1],
            dispatch.meta,
            uniform(args.len()),
            &dispatch.values[2..],
            false,
        )[0];
        self.b.ins().jump(join, &[r.into()]);

        self.b.switch_to_block(dispatch.slow);
        let helper = self.func_ref(self.imports.apply(args.len()));
        let call = self.b.ins().call(helper, &dispatch.values);
        let r = self.b.inst_results(call)[0];
        self.b.ins().jump(join, &[r.into()]);

        self.b.switch_to_block(join);
        self.bind(var, result);
        let point = self.point(block, position);
        let after = match point.landing {
            Some(_) => After::Land { result: var },
            None => After::Call,
        };
        self.after_unwind_point(&Point::from(point), after);
    }

    /// 末尾の`apply callee(args)`。`apply`と同じに分け、どちらの道も末尾で呼ぶ。
    pub(super) fn tail_apply(&mut self, callee: Atom, args: &[Atom]) {
        let dispatch = self.dispatch(callee, args);

        self.b.switch_to_block(dispatch.exact);
        call_closure_code(
            &mut self.b,
            self.ctx,
            dispatch.values[1],
            dispatch.meta,
            uniform(args.len()),
            &dispatch.values[2..],
            true,
        );

        self.b.switch_to_block(dispatch.slow);
        let helper = self.func_ref(self.imports.apply(args.len()));
        self.b.ins().return_call(helper, &dispatch.values);
    }

    /// 節の呼び出し`call`。`bound`は、`block`の`position`番目の文で束縛する変数で、`None`なら末尾の位置である。節の
    /// クロージャのコードを操作のシグネチャで呼ぶ。戻る操作の呼び出しの後には`Call`の確かめが続き、末尾の位置では末尾
    /// 呼び出しで、その結果は呼ぶ側の結果と等しくなければならない。`never`の操作の節は束縛のReprの値を返さないので、
    /// どちらの位置でも末尾でなく呼び、その後に`abort`のバイトを確かめる。冷たい側は地点の後始末で、熱い側はtrapする。
    /// 節が中断を始めずに戻ったからである。ブロックの残りは前のブロックのないブロックで続け、変数を仮の値に束縛する
    /// (docs/spec/native.mdの「関数の値」)。
    pub(super) fn clause(&mut self, call: &Call, bound: Option<(BlockId, usize, &[VarId])>) {
        let Call::Clause {
            effect,
            op,
            callee,
            args,
        } = call
        else {
            unreachable!("`{call:?}` is not a clause call")
        };
        let program = self.callees.program;
        let operation = &program.effects[*effect as usize].operations[*op as usize];
        let sig = clause_signature(program, *effect, *op);
        let clo = self.atom(*callee, Repr::TObj);
        let reprs = std::iter::once(Repr::Obj).chain(operation.params.iter().copied());
        let values = self.atoms(args, reprs);
        let meta = mem::load_meta(&mut self.b, clo);
        if operation.ret == OpRet::Never {
            call_closure_code(&mut self.b, self.ctx, clo, meta, sig, &values, false);
            let (cleanup, hot) = unwind::if_aborting(&mut self.b, self.ctx, &[]);
            match bound {
                Some((block, position, _)) => {
                    let point = self.point(block, position);
                    self.fill_cleanup(cleanup, &Point::from(point));
                }
                // 末尾の位置ではフレームは何も持たない
                None => self.fill_cleanup(
                    cleanup,
                    &Point {
                        owned: &[],
                        tokens: &[],
                        landing: None,
                    },
                ),
            }
            self.b.switch_to_block(hot);
            self.b.ins().trap(TrapCode::unwrap_user(1));
            if let Some((_, _, vars)) = bound {
                let dead = self.b.create_block();
                self.b.switch_to_block(dead);
                for &var in vars {
                    let value = unwind::placeholder(&mut self.b, self.function.repr(var));
                    self.bind(var, value);
                }
            }
            return;
        }
        match bound {
            Some((block, position, vars)) => {
                let results =
                    call_closure_code(&mut self.b, self.ctx, clo, meta, sig, &values, false);
                assert_eq!(
                    results.len(),
                    vars.len(),
                    "`{}` binds as many values as the clause returns",
                    self.function.name
                );
                for (&var, value) in vars.iter().zip(results) {
                    self.bind(var, value);
                }
                let point = self.point(block, position);
                self.after_unwind_point(&Point::from(point), After::Call);
            }
            None => {
                // Craneliftのverifierも結果の違う末尾呼び出しを断るが、そのフラグには頼らず、どのビルドでも確かめる
                assert_eq!(
                    sig.returns, self.b.func.signature.returns,
                    "a tail clause call of `{}.{}` returns other values than `{}`",
                    program.effects[*effect as usize].name, operation.name, self.function.name
                );
                call_closure_code(&mut self.b, self.ctx, clo, meta, sig, &values, true);
            }
        }
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
