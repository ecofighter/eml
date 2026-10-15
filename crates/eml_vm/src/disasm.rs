//! `CodeBase` を関数ごとのテキストにする。コード生成のテストのスナップショットと、誤りの調べに使う。

use std::fmt::Write as _;

use eml_extern::Extern;
use eml_rt::Mode;

use crate::code::CodeBase;
use crate::op::{Op, Pc};

pub fn disassemble<M: Mode>(code: &CodeBase<M>) -> String {
    let mut out = String::new();
    for f in &code.functions {
        writeln!(
            out,
            "fn {} (arity {}, frame {})",
            f.name, f.arity, f.frame_size
        )
        .unwrap();
        for pc in f.start..f.end {
            writeln!(out, "  {pc:>4}  {}", line(code, pc)).unwrap();
        }
    }
    out
}

fn slots(list: &[u32]) -> String {
    list.iter()
        .map(|slot| format!("s{slot}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn line<M: Mode>(code: &CodeBase<M>, pc: Pc) -> String {
    let ops = |at: u32| &code.operands[at as usize..];
    let args = |at: u32, skip: usize| {
        let o = ops(at);
        let n = o[skip - 1] as usize;
        slots(&o[skip..skip + n])
    };
    let name = |f: u32| code.functions[f as usize].name.as_str();
    let bin = |name: &str, dst: u16, a: u16, b: u16| format!("{name} s{dst}, s{a}, s{b}");
    let mut text = match code.code[pc as usize] {
        Op::Move { dst, src } => format!("move s{dst}, s{src}"),
        Op::LoadRaw { dst, value } => format!("load_raw s{dst}, {value}"),
        Op::LoadImm { dst, value } => format!("load_imm s{dst}, {value}"),
        Op::LoadConst { dst, index } => format!("load_const s{dst}, c{index}"),
        Op::Goto { target } => format!("goto {target}"),
        Op::Ret { src } => format!("ret s{src}"),
        Op::Call { dst, at } => format!("call s{dst} = {}({})", name(ops(at)[0]), args(at, 2)),
        Op::TailCall { at } => format!("tail_call {}({})", name(ops(at)[0]), args(at, 2)),
        Op::Finish => "finish".to_string(),
        Op::ApplyRest => "apply_rest".to_string(),
        Op::IntAdd { dst, a, b } => bin("int_add", dst, a, b),
        Op::IntSub { dst, a, b } => bin("int_sub", dst, a, b),
        Op::IntMul { dst, a, b } => bin("int_mul", dst, a, b),
        Op::IntDiv { dst, a, b } => bin("int_div", dst, a, b),
        Op::IntRem { dst, a, b } => bin("int_rem", dst, a, b),
        Op::IntNeg { dst, a } => format!("int_neg s{dst}, s{a}"),
        Op::IntEq { dst, a, b } => bin("int_eq", dst, a, b),
        Op::IntNe { dst, a, b } => bin("int_ne", dst, a, b),
        Op::IntLt { dst, a, b } => bin("int_lt", dst, a, b),
        Op::IntLe { dst, a, b } => bin("int_le", dst, a, b),
        Op::IntGt { dst, a, b } => bin("int_gt", dst, a, b),
        Op::IntGe { dst, a, b } => bin("int_ge", dst, a, b),
        Op::IntCompare { dst, a, b } => bin("int_compare", dst, a, b),
        Op::BoolEq { dst, a, b } => bin("bool_eq", dst, a, b),
        Op::BoolNe { dst, a, b } => bin("bool_ne", dst, a, b),
        Op::Extern { dst, at } => format!(
            "extern s{dst} = {}({})",
            Extern::ALL[ops(at)[0] as usize].row().name,
            args(at, 2)
        ),
    };
    if let Some(loc) = code.location(pc) {
        write!(
            text,
            " @{}:{}:{}",
            code.files[loc.file as usize], loc.line, loc.column
        )
        .unwrap();
    }
    text
}
