//! ネイティブのコードが下ろすstdの行と、その下ろし方 (docs/spec/native.mdの「断る形」)。ほかのstdの行は
//! `unsupported`が断る。

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use eml_extern::Extern;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inline {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Neg,
    Cmp(IntCC),
    Compare,
    Move,
    FAdd,
    FSub,
    FMul,
    FDiv,
    FNeg,
    FCmp(FloatCC),
    BoolEq,
    BoolNe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lowering {
    Inline(Inline),
    Called,
}

/// CLIFの演算に下ろす行。VMの算術と比較の専用の命令から`FloatMod`を除いたものである。Craneliftの`LibCall`には
/// `fmod`がないので、`FloatMod`は呼ぶ行にする。
pub const INLINE: &[(Extern, Inline)] = &[
    (Extern::IntAdd, Inline::Add),
    (Extern::IntSub, Inline::Sub),
    (Extern::IntMul, Inline::Mul),
    (Extern::IntDiv, Inline::Div),
    (Extern::ByteDiv, Inline::Div),
    (Extern::IntMod, Inline::Rem),
    (Extern::ByteMod, Inline::Rem),
    (Extern::IntNeg, Inline::Neg),
    (Extern::IntEq, Inline::Cmp(IntCC::Equal)),
    (Extern::IntNe, Inline::Cmp(IntCC::NotEqual)),
    (Extern::IntLt, Inline::Cmp(IntCC::SignedLessThan)),
    (Extern::IntLe, Inline::Cmp(IntCC::SignedLessThanOrEqual)),
    (Extern::IntGt, Inline::Cmp(IntCC::SignedGreaterThan)),
    (Extern::IntGe, Inline::Cmp(IntCC::SignedGreaterThanOrEqual)),
    (Extern::CharEq, Inline::Cmp(IntCC::Equal)),
    (Extern::CharNe, Inline::Cmp(IntCC::NotEqual)),
    (Extern::CharLt, Inline::Cmp(IntCC::SignedLessThan)),
    (Extern::CharLe, Inline::Cmp(IntCC::SignedLessThanOrEqual)),
    (Extern::CharGt, Inline::Cmp(IntCC::SignedGreaterThan)),
    (Extern::CharGe, Inline::Cmp(IntCC::SignedGreaterThanOrEqual)),
    (Extern::ByteEq, Inline::Cmp(IntCC::Equal)),
    (Extern::ByteNe, Inline::Cmp(IntCC::NotEqual)),
    (Extern::ByteLt, Inline::Cmp(IntCC::SignedLessThan)),
    (Extern::ByteLe, Inline::Cmp(IntCC::SignedLessThanOrEqual)),
    (Extern::ByteGt, Inline::Cmp(IntCC::SignedGreaterThan)),
    (Extern::ByteGe, Inline::Cmp(IntCC::SignedGreaterThanOrEqual)),
    (Extern::IntCompare, Inline::Compare),
    (Extern::CharCompare, Inline::Compare),
    (Extern::ByteCompare, Inline::Compare),
    (Extern::CharToInt, Inline::Move),
    (Extern::ByteToInt, Inline::Move),
    (Extern::CharOfByte, Inline::Move),
    (Extern::FloatAdd, Inline::FAdd),
    (Extern::FloatSub, Inline::FSub),
    (Extern::FloatMul, Inline::FMul),
    (Extern::FloatDiv, Inline::FDiv),
    (Extern::FloatNeg, Inline::FNeg),
    (Extern::FloatEq, Inline::FCmp(FloatCC::Equal)),
    (Extern::FloatNe, Inline::FCmp(FloatCC::NotEqual)),
    (Extern::FloatLt, Inline::FCmp(FloatCC::LessThan)),
    (Extern::FloatLe, Inline::FCmp(FloatCC::LessThanOrEqual)),
    (Extern::FloatGt, Inline::FCmp(FloatCC::GreaterThan)),
    (Extern::FloatGe, Inline::FCmp(FloatCC::GreaterThanOrEqual)),
    (Extern::BoolEq, Inline::BoolEq),
    (Extern::BoolNe, Inline::BoolNe),
];

/// エントリ`eml_x_<Variant>`の呼び出しに下ろす行。
pub const CALLED: &[Extern] = &[
    Extern::Println,
    Extern::Eprintln,
    Extern::IntShow,
    Extern::StrShow,
    Extern::FloatShow,
    Extern::StrConcat,
    Extern::StrEq,
    Extern::StrCompare,
    Extern::FloatMod,
    Extern::Exit,
    Extern::Panic,
];

pub fn lowering(ext: Extern) -> Option<Lowering> {
    if let Some(&(_, inline)) = INLINE.iter().find(|(row, _)| *row == ext) {
        Some(Lowering::Inline(inline))
    } else if CALLED.contains(&ext) {
        Some(Lowering::Called)
    } else {
        None
    }
}
