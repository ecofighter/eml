//! エントリのstubが呼ぶthunk (docs/spec/native.md の「`eml_enter`」)。emlのコードは`tail`の規約で、stubはそれを直接呼べない。
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlagsData, Signature, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Linkage, Module};
use eml_nrt::Kind;

use crate::abi::{Built, clif, tail_signature};

/// `thunk(ctx, entry, argv)`。引数を`argv`から読み、`tail`の関数`entry`を呼び、結果を`argv`に書く。thunkは引数の数ごとで
/// なく、種類の並びごとに1つ作る。JITのホストの記号の表のために、thunkのコードのバイト数も返す。
pub fn define_thunk<M: Module>(
    module: &mut M,
    name: &str,
    params: &[Kind],
    rets: &[Kind],
) -> Built<(FuncId, u32)> {
    let mut sig = Signature::new(module.isa().default_call_conv());
    for _ in 0..3 {
        sig.params.push(AbiParam::new(types::I64));
    }
    let id = module.declare_function(name, Linkage::Local, &sig)?;
    let mut ctx = module.make_context();
    ctx.func.signature = sig;
    let mut fctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
    let block = b.create_block();
    b.append_block_params_for_function_params(block);
    b.switch_to_block(block);
    let (c, entry, argv) = (
        b.block_params(block)[0],
        b.block_params(block)[1],
        b.block_params(block)[2],
    );
    let flags = MemFlagsData::trusted();
    let mut args = vec![c];
    for (i, &kind) in params.iter().enumerate() {
        args.push(b.ins().load(clif(kind), flags, argv, 8 * i as i32));
    }
    let callee = b.import_signature(tail_signature(params, rets));
    let call = b.ins().call_indirect(callee, entry, &args);
    let results = b.inst_results(call).to_vec();
    for (i, value) in results.into_iter().enumerate() {
        b.ins().store(flags, value, argv, 8 * i as i32);
    }
    b.ins().return_(&[]);
    b.seal_all_blocks();
    b.finalize(module.target_config());
    module.define_function(id, &mut ctx)?;
    let size = ctx
        .compiled_code()
        .expect("a defined thunk has its code")
        .code_buffer()
        .len();
    module.clear_context(&mut ctx);
    Ok((
        id,
        u32::try_from(size).expect("a thunk's code fits in 4 GiB"),
    ))
}
