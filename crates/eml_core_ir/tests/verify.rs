//! 手で組んだ Core IR で、verifier が正しいものを受け入れ、壊れたものを拒むことを確かめる (docs/spec/core-ir.md)。

use eml_core_ir::{
    Atom, CExpr, CExprId, Call, CoreFn, EffectInfo, FnIdx, JoinId, OperationInfo, PrimOp, Program,
    Rhs, VarId, VarInfo, verify, verify_scopes,
};
use eml_test_support::ir::{boxed, program, unboxed, var};

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
    verify(&program(functions, 0, &["s"])).map_err(|error| error.to_string())
}

fn check_scopes(functions: Vec<CoreFn>) -> Result<(), String> {
    verify_scopes(&program(functions, 0, &["s"])).map_err(|error| error.to_string())
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
            args: vec![var(2)],
        },
        CExpr::Let {
            var: VarId(2),
            rhs: Rhs::ConstString(0),
            body: CExprId(2),
        },
        CExpr::Jump {
            join: JoinId(0),
            args: vec![var(1)],
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
        params: vec![VarId(3)],
        captures: vec![VarId(1)],
        body: CExprId(1),
        scope: CExprId(switch),
    });
    let join = exprs.len() as u32 - 1;
    let vars = vec![unboxed("b"), boxed("s"), boxed("s"), boxed("t"), boxed("t")];
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
    let twice = function("twice", 1, vec![boxed("s"), boxed("t")], exprs, &[]);
    assert_eq!(check(vec![twice]), Ok(()));
}

#[test]
fn a_join_point_is_accepted() {
    assert_eq!(check(vec![pick(true)]), Ok(()));
}

#[test]
fn a_tail_call_that_takes_every_owned_value_is_accepted() {
    let id = function("id", 1, vec![boxed("s")], vec![CExpr::Return(var(0))], &[]);
    let exprs = vec![CExpr::TailCall(Call::Direct(FnIdx(0), vec![var(0)]))];
    let caller = function("caller", 1, vec![boxed("s")], exprs, &[]);
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
    let f = function("f", 0, vec![boxed("s")], exprs, &[]);
    assert_eq!(
        check(vec![f]),
        Err("`s0` is bound twice in `f`".to_string())
    );
}

#[test]
fn a_variable_used_outside_its_scope_is_rejected() {
    let f = function("f", 0, vec![boxed("s")], vec![CExpr::Return(var(0))], &[]);
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
            args: vec![Atom::Int(1)],
        },
        CExpr::Jump {
            join: JoinId(0),
            args: vec![Atom::Int(2)],
        },
        CExpr::Join {
            join: JoinId(0),
            params: vec![VarId(0)],
            captures: vec![],
            body: CExprId(0),
            scope: CExprId(1),
        },
    ];
    let f = function("f", 0, vec![unboxed("t")], exprs, &[2]);
    assert_eq!(
        check(vec![f]),
        Err("a jump to `j0` is outside its scope in `f`".to_string())
    );
}

#[test]
fn a_use_after_a_move_is_rejected() {
    let exprs = vec![CExpr::Return(var(1)), concat(1, 0, 0, 0)];
    let twice = function("twice", 1, vec![boxed("s"), boxed("t")], exprs, &[]);
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
        vec![boxed("s")],
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
    let ignore = function("ignore", 1, vec![boxed("s")], exprs, &[]);
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
            args: vec![Atom::Int(1)],
        },
        CExpr::Join {
            join: JoinId(0),
            params: vec![VarId(1)],
            captures: vec![],
            body: CExprId(0),
            scope: CExprId(1),
        },
    ];
    let f = function("f", 1, vec![boxed("s"), unboxed("t")], exprs, &[2]);
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

/// `two s = let u = "s" in join j0(a, b) [] { let c = a ++ b; return c } in jump j0(s, u)` の Perceus の後の形。
/// `passed` を変えて、`jump` が渡す値の数を変える。
fn two_values(passed: Vec<Atom>) -> CoreFn {
    let exprs = vec![
        CExpr::Return(var(4)),
        concat(4, 2, 3, 0),
        CExpr::Jump {
            join: JoinId(0),
            args: passed,
        },
        CExpr::Let {
            var: VarId(1),
            rhs: Rhs::ConstString(0),
            body: CExprId(2),
        },
        CExpr::Join {
            join: JoinId(0),
            params: vec![VarId(2), VarId(3)],
            captures: vec![],
            body: CExprId(1),
            scope: CExprId(3),
        },
    ];
    let vars = vec![boxed("s"), boxed("u"), boxed("a"), boxed("b"), boxed("c")];
    function("two", 1, vars, exprs, &[4])
}

#[test]
fn a_join_point_with_two_parameters_is_accepted() {
    // 本体は2つの引数をどちらも所有して始まり、`jump` は渡す値の所有権を渡す
    assert_eq!(check(vec![two_values(vec![var(0), var(1)])]), Ok(()));
}

#[test]
fn a_jump_with_the_wrong_number_of_values_is_rejected() {
    assert_eq!(
        check(vec![two_values(vec![var(0)])]),
        Err("a jump to `j0` passes 1 values, but its join takes 2 in `two`".to_string())
    );
}

#[test]
fn a_direct_call_with_the_wrong_number_of_arguments_is_rejected() {
    let g = function("g", 1, vec![unboxed("a")], vec![CExpr::Return(var(0))], &[]);
    let exprs = vec![
        CExpr::Return(var(0)),
        CExpr::Let {
            var: VarId(0),
            rhs: Rhs::call(Call::Direct(FnIdx(0), vec![Atom::Int(1), Atom::Int(2)])),
            body: CExprId(0),
        },
    ];
    let f = function("f", 0, vec![unboxed("t")], exprs, &[]);
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
    function("g", 1, vec![boxed("s")], vec![CExpr::Return(var(0))], &[])
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
    let f = function("f", 1, vec![boxed("s"), boxed("t"), boxed("t")], exprs, &[]);
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
    let f = function("f", 1, vec![boxed("s"), boxed("t")], exprs, &[]);
    assert_eq!(
        check(vec![identity(), f]),
        Err("a call saves [s0] but owns [] in `f`".to_string())
    );
}

#[test]
fn a_variable_not_saved_by_a_call_is_out_of_scope_after_it() {
    let k = function("k", 1, vec![unboxed("a")], vec![CExpr::Return(var(0))], &[]);
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
    let f = function(
        "f",
        1,
        vec![unboxed("n"), unboxed("t"), unboxed("t")],
        exprs,
        &[],
    );
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
            args: vec![var(3)],
        },
        CExpr::Let {
            var: VarId(3),
            rhs: Rhs::call(Call::Direct(FnIdx(0), vec![])),
            body: CExprId(2),
        },
        CExpr::Join {
            join: JoinId(0),
            params: vec![VarId(1)],
            captures: vec![VarId(0)],
            body: CExprId(1),
            scope: CExprId(3),
        },
    ];
    let f = function(
        "f",
        1,
        vec![unboxed("n"), unboxed("t"), unboxed("t"), unboxed("t")],
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
    let f = function("f", 1, vec![boxed("s"), boxed("t"), boxed("t")], exprs, &[]);
    assert_eq!(
        check(vec![identity(), f]),
        Err("a call saves [s0, s0] but owns [s0] in `f`".to_string())
    );
}

fn check_with_effects(functions: Vec<CoreFn>, effects: Vec<EffectInfo>) -> Result<(), String> {
    let program = Program {
        effects,
        ..program(functions, 0, &[])
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
        vec![boxed("c"), boxed("c"), unboxed("t")],
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
        vec![unboxed("p")],
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
        vec![unboxed("x"), boxed("k")],
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
        vec![boxed("s"), unboxed("t")],
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
        vec![boxed("s"), unboxed("t"), unboxed("u")],
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

#[test]
fn a_join_body_that_uses_a_variable_missing_from_its_captures_is_rejected() {
    // f n = join j0(t) [] { let u = n + t; return u } in jump j0(1)
    let exprs = vec![
        CExpr::Return(var(2)),
        CExpr::Let {
            var: VarId(2),
            rhs: Rhs::Prim(PrimOp::IntAdd, vec![var(0), var(1)]),
            body: CExprId(0),
        },
        CExpr::Jump {
            join: JoinId(0),
            args: vec![Atom::Int(1)],
        },
        CExpr::Join {
            join: JoinId(0),
            params: vec![VarId(1)],
            captures: vec![],
            body: CExprId(1),
            scope: CExprId(2),
        },
    ];
    let f = function(
        "f",
        1,
        vec![unboxed("n"), unboxed("t"), unboxed("u")],
        exprs,
        &[3],
    );
    assert_eq!(
        check(vec![f]),
        Err("`n0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_capture_out_of_scope_at_its_join_is_rejected() {
    // f n = join j0(t) [x] { return t } in jump j0(n)。`x` はどこでも束縛していない
    let exprs = vec![
        CExpr::Return(var(1)),
        CExpr::Jump {
            join: JoinId(0),
            args: vec![var(0)],
        },
        CExpr::Join {
            join: JoinId(0),
            params: vec![VarId(1)],
            captures: vec![VarId(2)],
            body: CExprId(0),
            scope: CExprId(1),
        },
    ];
    let f = function(
        "f",
        1,
        vec![unboxed("n"), unboxed("t"), unboxed("x")],
        exprs,
        &[2],
    );
    assert_eq!(
        check(vec![f]),
        Err("`j0` captures `x2`, which is not in scope in `f`".to_string())
    );
}

#[test]
fn captures_out_of_order_are_rejected() {
    let exprs = vec![
        CExpr::Return(var(2)),
        CExpr::Jump {
            join: JoinId(0),
            args: vec![Atom::Int(1)],
        },
        CExpr::Join {
            join: JoinId(0),
            params: vec![VarId(2)],
            captures: vec![VarId(1), VarId(0)],
            body: CExprId(0),
            scope: CExprId(1),
        },
    ];
    let f = function(
        "f",
        2,
        vec![unboxed("a"), unboxed("b"), unboxed("t")],
        exprs,
        &[2],
    );
    assert_eq!(
        check(vec![f]),
        Err("the captures of `j0` are not in increasing order in `f`".to_string())
    );
}

#[test]
fn an_unused_capture_released_by_the_body_is_accepted() {
    // f s = join j0(t) [s] { decref s; return t } in jump j0(1)。`captures` は最小でなくてよい
    let exprs = vec![
        CExpr::Return(var(1)),
        CExpr::Decref {
            var: VarId(0),
            body: CExprId(0),
        },
        CExpr::Jump {
            join: JoinId(0),
            args: vec![Atom::Int(1)],
        },
        CExpr::Join {
            join: JoinId(0),
            params: vec![VarId(1)],
            captures: vec![VarId(0)],
            body: CExprId(1),
            scope: CExprId(2),
        },
    ];
    let f = function("f", 1, vec![boxed("s"), unboxed("t")], exprs, &[3]);
    assert_eq!(check(vec![f]), Ok(()));
}

#[test]
fn scopes_accept_a_value_used_twice_without_dup() {
    // Perceus より前の IR には `dup` がないので、所有は数えない
    let exprs = vec![CExpr::Return(var(1)), concat(1, 0, 0, 0)];
    let twice = function("twice", 1, vec![boxed("s"), boxed("t")], exprs, &[]);
    assert_eq!(check_scopes(vec![twice]), Ok(()));
}

#[test]
fn scopes_accept_a_join_point_before_perceus() {
    assert_eq!(check_scopes(vec![pick(false)]), Ok(()));
}

#[test]
fn scopes_keep_variables_in_scope_after_a_call() {
    // `saved` は Perceus が決めるので、その前は呼び出しの後で範囲を区切り直さない
    let k = function("k", 1, vec![unboxed("a")], vec![CExpr::Return(var(0))], &[]);
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
    let f = function(
        "f",
        1,
        vec![unboxed("n"), unboxed("t"), unboxed("t")],
        exprs,
        &[],
    );
    assert_eq!(check_scopes(vec![k, f]), Ok(()));
}

#[test]
fn scopes_reject_a_dup() {
    let exprs = vec![
        CExpr::Return(var(1)),
        concat(1, 0, 0, 0),
        CExpr::Dup {
            var: VarId(0),
            body: CExprId(1),
        },
    ];
    let twice = function("twice", 1, vec![boxed("s"), boxed("t")], exprs, &[]);
    assert_eq!(
        check_scopes(vec![twice]),
        Err("`s0` is duplicated before Perceus in `twice`".to_string())
    );
}

#[test]
fn scopes_reject_a_decref() {
    let exprs = vec![
        CExpr::Return(Atom::Int(1)),
        CExpr::Decref {
            var: VarId(0),
            body: CExprId(0),
        },
    ];
    let ignore = function("ignore", 1, vec![boxed("s")], exprs, &[]);
    assert_eq!(
        check_scopes(vec![ignore]),
        Err("`s0` is released before Perceus in `ignore`".to_string())
    );
}

#[test]
fn scopes_reject_a_saved_list() {
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
    let f = function("f", 1, vec![boxed("s"), boxed("t")], exprs, &[]);
    assert_eq!(
        check_scopes(vec![identity(), f]),
        Err("a call saves [s0] before Perceus in `f`".to_string())
    );
}

#[test]
fn scopes_reject_a_variable_used_outside_its_scope() {
    let f = function("f", 0, vec![boxed("s")], vec![CExpr::Return(var(0))], &[]);
    assert_eq!(
        check_scopes(vec![f]),
        Err("`s0` is used outside its scope in `f`".to_string())
    );
}
