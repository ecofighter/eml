//! LIRをCLIFに下ろす。どの関数を定義するよりも前にすべての関数を宣言するので、本体はプログラムのどの関数も名指せる
//! (docs/spec/native.md の「関数の形」)。
mod function;
pub mod names;
pub mod rows;

use cranelift_codegen::ir::{Signature, UserFuncName};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Linkage, Module};
use eml_lir::{LirFn, Program, Repr};
use eml_nrt::Kind;

use crate::abi::{Built, tail_signature};

pub struct Defined {
    pub id: FuncId,
    pub size: u32,
    /// コンパイルの前のCLIF。求められたときだけ残す。
    pub clif: Option<String>,
}

pub fn kind(repr: Repr) -> Kind {
    match repr {
        Repr::Float => Kind::F64,
        Repr::Obj | Repr::TObj | Repr::Int | Repr::Enum | Repr::Unit | Repr::Token => Kind::I64,
        Repr::Struct(_) => unreachable!("flattening leaves no struct value"),
    }
}

pub fn params(function: &LirFn) -> Vec<Kind> {
    function
        .params()
        .iter()
        .map(|&var| kind(function.repr(var)))
        .collect()
}

pub fn rets(function: &LirFn) -> Vec<Kind> {
    function.rets.iter().map(|&(repr, _)| kind(repr)).collect()
}

pub fn signature_of(function: &LirFn) -> Signature {
    tail_signature(&params(function), &rets(function))
}

pub fn program<M: Module>(
    module: &mut M,
    program: &Program,
    keep_clif: bool,
) -> Built<Vec<Defined>> {
    let mut ids = Vec::with_capacity(program.functions.len());
    for function in &program.functions {
        ids.push(module.declare_function(
            &names::function(&function.name),
            Linkage::Local,
            &signature_of(function),
        )?);
    }
    let mut ctx = module.make_context();
    let mut fctx = FunctionBuilderContext::new();
    let mut defined = Vec::with_capacity(ids.len());
    for (function, id) in program.functions.iter().zip(ids) {
        ctx.func.signature = signature_of(function);
        ctx.func.name = UserFuncName::testcase(names::function(&function.name));
        let b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        function::lower(b, function, module.target_config());
        let clif = keep_clif.then(|| ctx.func.display().to_string());
        module.define_function(id, &mut ctx)?;
        let size = ctx
            .compiled_code()
            .expect("a defined function has its code")
            .code_buffer()
            .len();
        module.clear_context(&mut ctx);
        defined.push(Defined {
            id,
            size: u32::try_from(size).expect("a function's code fits in 4 GiB"),
            clif,
        });
    }
    Ok(defined)
}
