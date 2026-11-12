//! ネイティブのコードが下ろすstdの行と、その下ろし方 (docs/spec/native.mdの「断る形」)。ほかのstdの行は
//! `unsupported`が断る。

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{Block, InstBuilder, Value, types};
use cranelift_module::Module;
use eml_extern::{Extern, OpRet};
use eml_lir::{Atom, BlockId, Loc, VarId};

use super::function::FnLower;
use super::unwind::{After, Point, placeholder};

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

impl<M: Module> FnLower<'_, '_, M> {
    /// 呼ぶ行`ext`の文`let var = extern ext(args)`。引数を行の引数のReprで作り、`eml_x_<Variant>`を呼ぶ。`faults`の
    /// 行は呼び出しの後でfaultの語を確かめ、戻らない行は無条件に後始末へ進む (docs/spec/native.md の「externの失敗」)。
    pub(super) fn called_row(
        &mut self,
        block: BlockId,
        position: usize,
        var: VarId,
        ext: Extern,
        args: &[Atom],
        at: Option<Loc>,
    ) {
        let row = ext.row();
        let values: Vec<Value> = args
            .iter()
            .zip(row.params)
            .map(|(&arg, &repr)| self.atom(arg, repr))
            .collect();
        let result = self.call_entry(&format!("eml_x_{ext:?}"), &values);
        match row.ret {
            OpRet::Repr(_) => {
                self.bind(var, result.expect("a row with a result returns a word"));
                if row.faults {
                    let site = self.site(at);
                    let point = self.point(block, position);
                    let after = After::Faults {
                        site,
                        purity: row.purity,
                    };
                    self.after_unwind_point(&Point::from(point), after);
                }
            }
            OpRet::Never => {
                // `exit`は中断を始めるだけで失敗を置かない。ほかの戻らない行は置いた失敗を上げる
                let raise = match ext {
                    Extern::Exit => None,
                    _ => Some((self.site(at), row.purity)),
                };
                let point = self.point(block, position);
                self.after_unwind_point(&Point::from(point), After::Never { raise });
                let value = placeholder(&mut self.b, self.function.repr(var));
                self.bind(var, value);
            }
        }
    }
}

impl<M: Module> FnLower<'_, '_, M> {
    /// インラインの行`ext`の文`let var = extern ext(args)`。結果は行の共有の関数 (`eml_nrt::shared_rows`) とビット単位で
    /// 等しい。失敗する行は、失敗の条件で冷たい`arith`のブロックへ分岐する (docs/spec/native.mdの「行の下ろし方」)。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn inline_row(
        &mut self,
        block: BlockId,
        position: usize,
        var: VarId,
        ext: Extern,
        inline: Inline,
        args: &[Atom],
        at: Option<Loc>,
    ) {
        let row = ext.row();
        let values: Vec<Value> = args
            .iter()
            .zip(row.params)
            .map(|(&arg, &repr)| self.atom(arg, repr))
            .collect();
        let value = match (inline, &values[..]) {
            (Inline::Add | Inline::Sub | Inline::Mul, &[a, b]) => {
                let (r, overflowed) = match inline {
                    Inline::Add => self.b.ins().sadd_overflow(a, b),
                    Inline::Sub => self.b.ins().ssub_overflow(a, b),
                    _ => self.b.ins().smul_overflow(a, b),
                };
                let overflow = self.cold_block();
                let hot = self.branch_out(overflowed, overflow);
                self.arith_point(block, position, at, overflow, None, hot);
                r
            }
            (Inline::Div | Inline::Rem, &[a, b]) => {
                // `sdiv`と`srem`は0で割るとtrapし、`sdiv`は`MIN / -1`でもtrapするので、先に分岐する。`srem`は
                // `MIN % -1`を0にするが、共有の関数はそれを桁あふれにする
                let zero = self.b.ins().iconst(types::I64, 0);
                let by_zero = self.b.ins().icmp(IntCC::Equal, b, zero);
                let zero_block = self.cold_block();
                let next = self.branch_out(by_zero, zero_block);
                self.b.switch_to_block(next);
                let min = self.b.ins().iconst(types::I64, i64::MIN);
                let minus_one = self.b.ins().iconst(types::I64, -1);
                let a_min = self.b.ins().icmp(IntCC::Equal, a, min);
                let b_minus_one = self.b.ins().icmp(IntCC::Equal, b, minus_one);
                let overflowed = self.b.ins().band(a_min, b_minus_one);
                let overflow = self.cold_block();
                let hot = self.branch_out(overflowed, overflow);
                self.arith_point(block, position, at, overflow, Some(zero_block), hot);
                match inline {
                    Inline::Div => self.b.ins().sdiv(a, b),
                    _ => self.b.ins().srem(a, b),
                }
            }
            (Inline::Neg, &[a]) => {
                let min = self.b.ins().iconst(types::I64, i64::MIN);
                let overflowed = self.b.ins().icmp(IntCC::Equal, a, min);
                let overflow = self.cold_block();
                let hot = self.branch_out(overflowed, overflow);
                self.arith_point(block, position, at, overflow, None, hot);
                self.b.ins().ineg(a)
            }
            (Inline::Cmp(cc), &[a, b]) => {
                let r = self.b.ins().icmp(cc, a, b);
                self.b.ins().uextend(types::I64, r)
            }
            (Inline::Compare, &[a, b]) => {
                // `LT`、`EQ`、`GT`は0、1、2なので、`(a > b) + (a >= b)`がそのタグである
                let gt = self.b.ins().icmp(IntCC::SignedGreaterThan, a, b);
                let ge = self.b.ins().icmp(IntCC::SignedGreaterThanOrEqual, a, b);
                let gt = self.b.ins().uextend(types::I64, gt);
                let ge = self.b.ins().uextend(types::I64, ge);
                self.b.ins().iadd(gt, ge)
            }
            (Inline::Move, &[a]) => a,
            (Inline::FAdd | Inline::FSub | Inline::FMul | Inline::FDiv, &[a, b]) => {
                let r = match inline {
                    Inline::FAdd => self.b.ins().fadd(a, b),
                    Inline::FSub => self.b.ins().fsub(a, b),
                    Inline::FMul => self.b.ins().fmul(a, b),
                    _ => self.b.ins().fdiv(a, b),
                };
                self.canonical_nan(r)
            }
            (Inline::FNeg, &[a]) => {
                let r = self.b.ins().fneg(a);
                self.canonical_nan(r)
            }
            (Inline::FCmp(cc), &[a, b]) => {
                let r = self.b.ins().fcmp(cc, a, b);
                self.b.ins().uextend(types::I64, r)
            }
            (Inline::BoolEq | Inline::BoolNe, &[a, b]) => {
                let cc = match inline {
                    Inline::BoolEq => IntCC::Equal,
                    _ => IntCC::NotEqual,
                };
                let r = self.b.ins().icmp(cc, a, b);
                self.b.ins().uextend(types::I64, r)
            }
            _ => unreachable!("`{ext:?}` has a row of another arity"),
        };
        self.bind(var, value);
    }

    /// `condition`が真なら`exit`へ、偽なら新しいブロックへ分岐し、その新しいブロックを返す。
    fn branch_out(&mut self, condition: Value, exit: Block) -> Block {
        let next = self.b.create_block();
        self.b.ins().brif(condition, exit, &[], next, &[]);
        next
    }

    /// 失敗するインラインの行の巻き戻しの地点。今のブロックは`hot`への分岐で終わっている。
    fn arith_point(
        &mut self,
        block: BlockId,
        position: usize,
        at: Option<Loc>,
        overflow: Block,
        zero: Option<Block>,
        hot: Block,
    ) {
        let site = self.site(at);
        let point = self.point(block, position);
        self.after_unwind_point(
            &Point::from(point),
            After::Arith {
                site,
                overflow,
                zero,
                hot,
            },
        );
    }

    /// NaNを正のquiet NaN (`f64::NAN`) にそろえる。共有の関数の`canonical_nan`と同じく、どの対象でも結果のビットが
    /// 決まる。
    fn canonical_nan(&mut self, r: Value) -> Value {
        let nan = self.b.ins().fcmp(FloatCC::Unordered, r, r);
        let canonical = self.b.ins().f64const(f64::NAN);
        self.b.ins().select(nan, canonical, r)
    }
}
