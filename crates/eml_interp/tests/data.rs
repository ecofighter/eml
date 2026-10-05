//! 手で組んだ Core IR で、`data` の値の確保と、`Switch` の枝による分解を確かめる (docs/spec/core-ir.md)。共有された
//! 値の分解や、boxed な変数に入った引数のないコンストラクタは、ソースの `match` からは狙って作りにくいので、ここで
//! 組む。

use eml_core_ir::{Arm, Atom, CExpr, CExprId, CoreFn, IoOp, Rhs, VarId, VarInfo};
use eml_interp::{Fault, RuntimeError};
use eml_test_support::execute;
use eml_test_support::ir::{boxed, program, unboxed, var};

/// 引数のない入口の関数。`exprs` は子を親より先に並べ、最後の式を本体にする。
fn main_fn(vars: Vec<VarInfo>, exprs: Vec<CExpr>) -> CoreFn {
    CoreFn {
        name: "main".to_string(),
        params: Vec::new(),
        vars,
        body: CExprId(exprs.len() as u32 - 1),
        exprs,
        joins: Vec::new(),
    }
}

fn bind(var: u32, rhs: Rhs, body: u32) -> CExpr {
    CExpr::Let {
        var: VarId(var),
        rhs,
        body: CExprId(body),
    }
}

fn arm(tag: u32, fields: &[u32], body: u32) -> Arm {
    Arm {
        tag,
        fields: fields.iter().map(|&field| VarId(field)).collect(),
        body: CExprId(body),
    }
}

/// `let d = con #1(s)` を作り、`#1(x)` の枝で `x` を出力する。`shared` なら `Switch` の前で `d` を複製し、両方の
/// 枝で `d` を捨てる。
fn unpack(shared: bool) -> CoreFn {
    let vars = vec![boxed("s"), boxed("d"), boxed("x"), unboxed("o")];
    let exprs = if shared {
        vec![
            CExpr::Return(var(3)),
            CExpr::Decref {
                var: VarId(1),
                body: CExprId(0),
            },
            bind(3, Rhs::Io(IoOp::Println, vec![var(2)]), 1),
            CExpr::Return(Atom::Unit),
            CExpr::Decref {
                var: VarId(1),
                body: CExprId(3),
            },
            CExpr::Switch {
                scrutinee: var(1),
                arms: vec![arm(0, &[], 4), arm(1, &[2], 2)],
            },
            CExpr::Dup {
                var: VarId(1),
                body: CExprId(5),
            },
            bind(
                1,
                Rhs::Con {
                    tag: 1,
                    args: vec![var(0)],
                },
                6,
            ),
            bind(0, Rhs::ConstString(0), 7),
        ]
    } else {
        vec![
            CExpr::Return(var(3)),
            bind(3, Rhs::Io(IoOp::Println, vec![var(2)]), 0),
            CExpr::Return(Atom::Unit),
            CExpr::Switch {
                scrutinee: var(1),
                arms: vec![arm(0, &[], 2), arm(1, &[2], 1)],
            },
            bind(
                1,
                Rhs::Con {
                    tag: 1,
                    args: vec![var(0)],
                },
                3,
            ),
            bind(0, Rhs::ConstString(0), 4),
        ]
    };
    main_fn(vars, exprs)
}

#[test]
fn a_unique_value_is_unpacked_by_taking_its_fields() {
    let (stdout, result) = execute(program(vec![unpack(false)], 0, &["field"]), true);
    assert_eq!((stdout.as_str(), result), ("field\n", Ok(())));
}

#[test]
fn a_shared_value_is_unpacked_by_copying_its_fields() {
    let (stdout, result) = execute(program(vec![unpack(true)], 0, &["field"]), true);
    assert_eq!((stdout.as_str(), result), ("field\n", Ok(())));
}

#[test]
fn a_boxed_variable_holding_a_tag_takes_its_arm() {
    // 引数のないコンストラクタの値は、引数を持つコンストラクタのある型の変数 (boxed) にも即値で入る
    let vars = vec![boxed("d"), boxed("x"), boxed("s"), unboxed("o")];
    let exprs = vec![
        CExpr::Return(var(3)),
        bind(3, Rhs::Io(IoOp::Println, vec![var(2)]), 0),
        bind(2, Rhs::ConstString(0), 1),
        CExpr::Return(Atom::Unit),
        CExpr::Decref {
            var: VarId(1),
            body: CExprId(3),
        },
        CExpr::Switch {
            scrutinee: var(0),
            arms: vec![arm(0, &[], 2), arm(1, &[1], 4)],
        },
        bind(0, Rhs::Atom(Atom::Tag(0)), 5),
    ];
    let (stdout, result) = execute(program(vec![main_fn(vars, exprs)], 0, &["none"]), true);
    assert_eq!((stdout.as_str(), result), ("none\n", Ok(())));
}

#[test]
fn an_arm_with_a_different_number_of_fields_is_an_internal_error() {
    let vars = vec![boxed("s"), boxed("d"), boxed("x"), boxed("y")];
    let exprs = vec![
        CExpr::Return(Atom::Unit),
        CExpr::Return(Atom::Unit),
        CExpr::Switch {
            scrutinee: var(1),
            arms: vec![arm(0, &[], 0), arm(1, &[2, 3], 1)],
        },
        bind(
            1,
            Rhs::Con {
                tag: 1,
                args: vec![var(0)],
            },
            2,
        ),
        bind(0, Rhs::ConstString(0), 3),
    ];
    let (_, result) = execute(program(vec![main_fn(vars, exprs)], 0, &["field"]), true);
    assert_eq!(
        result,
        Err(RuntimeError::Fault {
            fault: Fault::Internal(
                "a switch arm binds a different number of fields than the value has"
            ),
            function: "main".to_string(),
        })
    );
}
