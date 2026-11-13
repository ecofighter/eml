//! Cの関数のCraneliftの署名 (docs/spec/vm.mdの「橋渡しの関数」)。橋渡しとネイティブのコードが同じ署名で呼ぶ。

use cranelift_codegen::ir::{AbiParam, Signature, types};
use cranelift_codegen::isa::CallConv;
use eml_extern::{CSignature, CType};

fn clif(ty: CType) -> types::Type {
    match ty {
        CType::I32 => types::I32,
        CType::I64 => types::I64,
        CType::F64 => types::F64,
    }
}

/// Cの型どおりの型を持つ素の`AbiParam`の署名。`I32`にも拡張の印を付けないので、呼び手は`ireduce`で絞った値を渡し、
/// `I32`の結果は明示の`sextend`で広げる。対応する対象 (x86_64のSysV、Win64、Appleのものを含むAAPCS64) では、上の
/// 32 bitの中身をCのABIが決めないためである。32 bitの整数に符号の拡張を求めるriscv64とs390xは、対応する対象ではない。
pub fn c_signature(signature: &CSignature, call_conv: CallConv) -> Signature {
    let mut sig = Signature::new(call_conv);
    sig.params
        .extend(signature.params.iter().map(|&ty| AbiParam::new(clif(ty))));
    sig.returns
        .extend(signature.ret.map(|ty| AbiParam::new(clif(ty))));
    sig
}
