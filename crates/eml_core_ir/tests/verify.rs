//! 手で組んだ Core IR で、verifier が正しいものを受け入れ、壊れたものを拒むことを確かめる (docs/spec/core-ir.md)。

use eml_core_ir::{
    Atom, CExpr, CExprId, Call, CoreFn, EffectInfo, FnIdx, JoinId, Linearity, OperationInfo,
    PrimOp, Program, Rhs, VarId, VarInfo, verify,
};

fn string(name: &str) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        linearity: Linearity::Unr,
        boxed: true,
    }
}

fn int(name: &str) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        linearity: Linearity::Unr,
        boxed: false,
    }
}

/// `exprs` は子を親より先に並べ、最後の式を本体にする。
fn function(
    name: &str,
    params: u32,
    vars: Vec<VarInfo>,
    exprs: Vec<CExpr>,
    joins: &[u32],
) -> CoreFn {
    CoreFn {
        name: name.to_string(),
        params: (0..params).map(VarId).collect(),
        vars,
        body: CExprId(exprs.len() as u32 - 1),
        exprs,
        joins: joins.iter().map(|&id| CExprId(id)).collect(),
    }
}

fn check(functions: Vec<CoreFn>) -> Result<(), String> {
    let program = Program {
        functions,
        entry: FnIdx(0),
        strings: vec!["s".to_string()],
        effects: Vec::new(),
    };
    verify(&program).map_err(|error| error.to_string())
}

fn var(n: u32) -> Atom {
    Atom::Var(VarId(n))
}

fn concat(var: u32, left: u32, right: u32, body: u32) -> CExpr {
    CExpr::Let {
        var: VarId(var),
        rhs: Rhs::Prim(PrimOp::StrConcat, vec![self::var(left), self::var(right)]),
        body: CExprId(body),
    }
}

/// `pick b s = let t = if b then s else "none" in t ++ s` の Perceus の後の形。
fn pick(dup_before_jump: bool) -> CoreFn {
    let mut exprs = vec![
        CExpr::Return(var(4)),
        concat(4, 3, 1, 0),
        CExpr::Jump {
            join: JoinId(0),
            arg: var(2),
        },
        CExpr::Let {
            var: VarId(2),
            rhs: Rhs::ConstString(0),
            body: CExprId(2),
        },
        CExpr::Jump {
            join: JoinId(0),
            arg: var(1),
        },
    ];
    let then_arm = if dup_before_jump {
        exprs.push(CExpr::Dup {
            var: VarId(1),
            body: CExprId(4),
        });
        5
    } else {
        4
    };
    exprs.push(CExpr::Switch {
        scrutinee: var(0),
        arms: vec![(0, CExprId(3)), (1, CExprId(then_arm))],
    });
    let switch = exprs.len() as u32 - 1;
    exprs.push(CExpr::Join {
        join: JoinId(0),
        param: VarId(3),
        captures: vec![VarId(1)],
        body: CExprId(1),
        scope: CExprId(switch),
    });
    let join = exprs.len() as u32 - 1;
    let vars = vec![int("b"), string("s"), string("s"), string("t"), string("t")];
    function("pick", 2, vars, exprs, &[join])
}

#[test]
fn a_duplicated_string_used_twice_is_accepted() {
    let exprs = vec![
        CExpr::Return(var(1)),
        concat(1, 0, 0, 0),
        CExpr::Dup {
            var: VarId(0),
            body: CExprId(1),
        },
    ];
    let twice = function("twice", 1, vec![string("s"), string("t")], exprs, &[]);
    assert_eq!(check(vec![twice]), Ok(()));
}

#[test]
fn a_join_point_is_accepted() {
    assert_eq!(check(vec![pick(true)]), Ok(()));
}

#[test]
fn a_tail_call_that_takes_every_owned_value_is_accepted() {
    let id = function("id", 1, vec![string("s")], vec![CExpr::Return(var(0))], &[]);
    let exprs = vec![CExpr::TailCall(Call::Direct(FnIdx(0), vec![var(0)]))];
    let caller = function("caller", 1, vec![string("s")], exprs, &[]);
    assert_eq!(check(vec![id, caller]), Ok(()));
}

#[test]
fn a_variable_bound_twice_is_rejected() {
    let exprs = vec![
        CExpr::Return(var(0)),
        CExpr::Let {
            var: VarId(0),
            rhs: Rhs::ConstString(0),
            body: CExprId(0),
        },
        CExpr::Let {
            var: VarId(0),
            rhs: Rhs::ConstString(0),
            body: CExprId(1),
        },
    ];
    let f = function("f", 0, vec![string("s")], exprs, &[]);
    assert_eq!(
        check(vec![f]),
        Err("`s0` is bound twice in `f`".to_string())
    );
}

#[test]
fn a_variable_used_outside_its_scope_is_rejected() {
    let f = function("f", 0, vec![string("s")], vec![CExpr::Return(var(0))], &[]);
    assert_eq!(
        check(vec![f]),
        Err("`s0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_jump_outside_its_join_scope_is_rejected() {
    let exprs = vec![
        CExpr::Jump {
            join: JoinId(0),
            arg: Atom::Int(1),
        },
        CExpr::Jump {
            join: JoinId(0),
            arg: Atom::Int(2),
        },
        CExpr::Join {
            join: JoinId(0),
            param: VarId(0),
            captures: vec![],
            body: CExprId(0),
            scope: CExprId(1),
        },
    ];
    let f = function("f", 0, vec![int("t")], exprs, &[2]);
    assert_eq!(
        check(vec![f]),
        Err("a jump to `j0` is outside its scope in `f`".to_string())
    );
}

#[test]
fn a_use_after_a_move_is_rejected() {
    let exprs = vec![CExpr::Return(var(1)), concat(1, 0, 0, 0)];
    let twice = function("twice", 1, vec![string("s"), string("t")], exprs, &[]);
    assert_eq!(
        check(vec![twice]),
        Err("`s0` is used after it was moved in `twice`".to_string())
    );
}

#[test]
fn a_missing_decref_is_rejected() {
    let ignore = function(
        "ignore",
        1,
        vec![string("s")],
        vec![CExpr::Return(Atom::Int(1))],
        &[],
    );
    assert_eq!(
        check(vec![ignore]),
        Err("`s0` is still owned at the end of the function in `ignore`".to_string())
    );
}

#[test]
fn a_double_decref_is_rejected() {
    let exprs = vec![
        CExpr::Return(Atom::Int(1)),
        CExpr::Decref {
            var: VarId(0),
            body: CExprId(0),
        },
        CExpr::Decref {
            var: VarId(0),
            body: CExprId(1),
        },
    ];
    let ignore = function("ignore", 1, vec![string("s")], exprs, &[]);
    assert_eq!(
        check(vec![ignore]),
        Err("`s0` is released after it was moved in `ignore`".to_string())
    );
}

#[test]
fn a_jump_that_owns_too_much_is_rejected() {
    let exprs = vec![
        CExpr::Return(var(1)),
        CExpr::Jump {
            join: JoinId(0),
            arg: Atom::Int(1),
        },
        CExpr::Join {
            join: JoinId(0),
            param: VarId(1),
            captures: vec![],
            body: CExprId(0),
            scope: CExprId(1),
        },
    ];
    let f = function("f", 1, vec![string("s"), int("t")], exprs, &[2]);
    assert_eq!(
        check(vec![f]),
        Err("a jump to `j0` owns [s0] but its join needs [] in `f`".to_string())
    );
}

#[test]
fn a_jump_that_owns_too_little_is_rejected() {
    assert_eq!(
        check(vec![pick(false)]),
        Err("a jump to `j0` owns [] but its join needs [s1] in `pick`".to_string())
    );
}

#[test]
fn a_direct_call_with_the_wrong_number_of_arguments_is_rejected() {
    let g = function("g", 1, vec![int("a")], vec![CExpr::Return(var(0))], &[]);
    let exprs = vec![
        CExpr::Return(var(0)),
        CExpr::Let {
            var: VarId(0),
            rhs: Rhs::call(Call::Direct(FnIdx(0), vec![Atom::Int(1), Atom::Int(2)])),
            body: CExprId(0),
        },
    ];
    let f = function("f", 0, vec![int("t")], exprs, &[]);
    assert_eq!(
        check(vec![g, f]),
        Err("a direct call to `g` passes 2 arguments, but it takes 1 in `f`".to_string())
    );
}

#[test]
fn a_long_run_of_if_statements_is_verified_in_linear_time() {
    // 文の `if` が続くと、join point の本体が長く連なる。範囲や枝ごとに変数の範囲を写すと、文の数の2乗の時間がかかる。
    // `lower` はデバッグビルドで毎回 verify するので、文の数に比例する時間で終わらなければならない
    let mut text = String::from("main : Unit -> <IO> Unit\nmain () =\n  let s = \"keep\"\n");
    text.push_str(&"  if True then println \"x\"\n".repeat(20000));
    text.push_str("  println s");
    let start = std::time::Instant::now();
    let program = eml_test_support::core(&text);
    assert_eq!(verify(&program), Ok(()));
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "took {elapsed:?}"
    );
}

/// `g s = s`。
fn identity() -> CoreFn {
    function("g", 1, vec![string("s")], vec![CExpr::Return(var(0))], &[])
}

#[test]
fn a_call_that_does_not_save_an_owned_variable_is_rejected() {
    // f s = dup s; let t = g s; s を退避しないまま t ++ s
    let exprs = vec![
        CExpr::Return(var(2)),
        concat(2, 1, 0, 0),
        CExpr::Let {
            var: VarId(1),
            rhs: Rhs::call(Call::Direct(FnIdx(0), vec![var(0)])),
            body: CExprId(1),
        },
        CExpr::Dup {
            var: VarId(0),
            body: CExprId(2),
        },
    ];
    let f = function(
        "f",
        1,
        vec![string("s"), string("t"), string("t")],
        exprs,
        &[],
    );
    assert_eq!(
        check(vec![identity(), f]),
        Err("a call saves [] but owns [s0] in `f`".to_string())
    );
}

#[test]
fn a_call_that_saves_a_variable_it_does_not_own_is_rejected() {
    let exprs = vec![
        CExpr::Return(var(1)),
        CExpr::Let {
            var: VarId(1),
            rhs: Rhs::Call {
                call: Call::Direct(FnIdx(0), vec![var(0)]),
                saved: vec![VarId(0)],
            },
            body: CExprId(0),
        },
    ];
    let f = function("f", 1, vec![string("s"), string("t")], exprs, &[]);
    assert_eq!(
        check(vec![identity(), f]),
        Err("a call saves [s0] but owns [] in `f`".to_string())
    );
}

#[test]
fn a_variable_not_saved_by_a_call_is_out_of_scope_after_it() {
    let k = function("k", 1, vec![int("a")], vec![CExpr::Return(var(0))], &[]);
    let exprs = vec![
        CExpr::Return(var(2)),
        CExpr::Let {
            var: VarId(2),
            rhs: Rhs::Prim(PrimOp::IntAdd, vec![var(0), var(1)]),
            body: CExprId(0),
        },
        CExpr::Let {
            var: VarId(1),
            rhs: Rhs::call(Call::Direct(FnIdx(0), vec![var(0)])),
            body: CExprId(1),
        },
    ];
    let f = function("f", 1, vec![int("n"), int("t"), int("t")], exprs, &[]);
    assert_eq!(
        check(vec![k, f]),
        Err("`n0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_jump_after_a_call_needs_the_variables_of_the_join_body_in_scope() {
    let z = function("z", 0, vec![], vec![CExpr::Return(Atom::Int(1))], &[]);
    let exprs = vec![
        CExpr::Return(var(2)),
        CExpr::Let {
            var: VarId(2),
            rhs: Rhs::Prim(PrimOp::IntAdd, vec![var(0), var(1)]),
            body: CExprId(0),
        },
        CExpr::Jump {
            join: JoinId(0),
            arg: var(3),
        },
        CExpr::Let {
            var: VarId(3),
            rhs: Rhs::call(Call::Direct(FnIdx(0), vec![])),
            body: CExprId(2),
        },
        CExpr::Join {
            join: JoinId(0),
            param: VarId(1),
            captures: vec![VarId(0)],
            body: CExprId(1),
            scope: CExprId(3),
        },
    ];
    let f = function(
        "f",
        1,
        vec![int("n"), int("t"), int("t"), int("t")],
        exprs,
        &[4],
    );
    assert_eq!(
        check(vec![z, f]),
        Err("`n0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_call_that_saves_a_variable_twice_is_rejected() {
    // 退避する変数が重なると、フレームが所有していない参照を、解放や複製のときに数えてしまう
    let exprs = vec![
        CExpr::Return(var(2)),
        concat(2, 1, 0, 0),
        CExpr::Let {
            var: VarId(1),
            rhs: Rhs::Call {
                call: Call::Direct(FnIdx(0), vec![var(0)]),
                saved: vec![VarId(0), VarId(0)],
            },
            body: CExprId(1),
        },
        CExpr::Dup {
            var: VarId(0),
            body: CExprId(2),
        },
    ];
    let f = function(
        "f",
        1,
        vec![string("s"), string("t"), string("t")],
        exprs,
        &[],
    );
    assert_eq!(
        check(vec![identity(), f]),
        Err("a call saves [s0, s0] but owns [s0] in `f`".to_string())
    );
}

fn check_with_effects(functions: Vec<CoreFn>, effects: Vec<EffectInfo>) -> Result<(), String> {
    let program = Program {
        functions,
        entry: FnIdx(0),
        strings: Vec::new(),
        effects,
    };
    verify(&program).map_err(|error| error.to_string())
}

fn ask_effect() -> Vec<EffectInfo> {
    vec![EffectInfo {
        name: "Ask".to_string(),
        operations: vec![OperationInfo {
            name: "ask".to_string(),
            resumable: true,
        }],
    }]
}

/// `handle ask 1 with | ask x k -> resume k x` を持ち上げた形。
fn handler_program() -> Vec<CoreFn> {
    let main = function(
        "main",
        0,
        vec![string("c"), string("c"), int("t")],
        vec![
            CExpr::Return(var(2)),
            CExpr::Let {
                var: VarId(2),
                rhs: Rhs::Call {
                    call: Call::Handle {
                        effect: 0,
                        body: var(0),
                        clauses: vec![var(1)],
                        ret: None,
                    },
                    saved: Vec::new(),
                },
                body: CExprId(0),
            },
            CExpr::Let {
                var: VarId(1),
                rhs: Rhs::MakeClosure(FnIdx(2), Vec::new()),
                body: CExprId(1),
            },
            CExpr::Let {
                var: VarId(0),
                rhs: Rhs::MakeClosure(FnIdx(1), Vec::new()),
                body: CExprId(2),
            },
        ],
        &[],
    );
    let body = function(
        "main$handle0",
        1,
        vec![int("p")],
        vec![CExpr::TailCall(Call::Perform {
            effect: 0,
            op: 0,
            args: vec![Atom::Int(1)],
        })],
        &[],
    );
    let clause = function(
        "main$handle0$ask",
        2,
        vec![int("x"), string("k")],
        vec![CExpr::TailCall(Call::Resume {
            k: var(1),
            arg: var(0),
        })],
        &[],
    );
    vec![main, body, clause]
}

#[test]
fn handlers_operations_and_resume_are_calls() {
    assert_eq!(check_with_effects(handler_program(), ask_effect()), Ok(()));
}

#[test]
fn a_handler_has_a_clause_for_each_operation() {
    let mut effects = ask_effect();
    effects[0].operations.push(OperationInfo {
        name: "tell".to_string(),
        resumable: true,
    });
    assert_eq!(
        check_with_effects(handler_program(), effects),
        Err(
            "a handler of `Ask` has clauses for 1 operations, but the effect has 2 in `main`"
                .to_string()
        )
    );
}

#[test]
fn perform_names_an_operation_of_its_effect() {
    let f = function(
        "f",
        0,
        Vec::new(),
        vec![CExpr::TailCall(Call::Perform {
            effect: 0,
            op: 1,
            args: Vec::new(),
        })],
        &[],
    );
    assert_eq!(
        check_with_effects(vec![f], ask_effect()),
        Err("`perform` names operation 1 of `Ask`, which has 1 operations in `f`".to_string())
    );
}

#[test]
fn drop_takes_the_ownership_of_its_value() {
    let once = function(
        "f",
        1,
        vec![string("s"), int("t")],
        vec![
            CExpr::Return(var(1)),
            CExpr::Let {
                var: VarId(1),
                rhs: Rhs::Drop(var(0)),
                body: CExprId(0),
            },
        ],
        &[],
    );
    assert_eq!(check_with_effects(vec![once], Vec::new()), Ok(()));
    let twice = function(
        "g",
        1,
        vec![string("s"), int("t"), int("u")],
        vec![
            CExpr::Return(var(2)),
            CExpr::Let {
                var: VarId(2),
                rhs: Rhs::Drop(var(0)),
                body: CExprId(0),
            },
            CExpr::Let {
                var: VarId(1),
                rhs: Rhs::Drop(var(0)),
                body: CExprId(1),
            },
        ],
        &[],
    );
    assert_eq!(
        check_with_effects(vec![twice], Vec::new()),
        Err("`s0` is used after it was moved in `g`".to_string())
    );
}
