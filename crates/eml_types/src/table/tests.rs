use super::*;
use crate::context::{Context, test_context};
use crate::kind::solve::{residual_of, solve};

/// Prelude だけのプログラムの表示名で型を書く。Prelude の名前は重ならないので、修飾しない。
fn shown(table: &Table, ty: Ty) -> String {
    table
        .export(ty)
        .display(&crate::test_program("").names)
        .to_string()
}

fn effect_named(program: &eml_hir::Program, name: &str) -> EffectId {
    program
        .effects()
        .find(|(_, effect)| effect.name == name)
        .map(|(id, _)| id)
        .unwrap()
}

#[test]
fn constructors_unify_only_with_themselves() {
    let context = test_context();
    let mut table = Table::new(&context);
    assert_eq!(table.unify(table.int, table.int), Ok(()));
    assert_eq!(
        table.unify(table.int, table.string),
        Err(UnifyError::Mismatch)
    );
    assert_eq!(table.unify(table.error, table.string), Ok(()));
}

#[test]
fn variables_are_bound_by_unification() {
    let context = test_context();
    let mut table = Table::new(&context);
    let v = table.fresh_var();
    let f = table.function(table.int, Row::pure(), v);
    let g = table.function(table.int, Row::pure(), table.bool);
    assert_eq!(table.unify(f, g), Ok(()));
    assert_eq!(shown(&table, v), "Bool");
}

#[test]
fn the_occurs_check_rejects_infinite_types() {
    let context = test_context();
    let mut table = Table::new(&context);
    let v = table.fresh_var();
    let f = table.function(v, Row::pure(), table.int);
    assert_eq!(table.unify(v, f), Err(UnifyError::Occurs));
}

#[test]
fn closed_rows_unify_regardless_of_order() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let a = Row::closed(vec![Label::plain(io)]);
    assert_eq!(table.unify_row(&a, &a.clone()), Ok(()));
    assert_eq!(
        table.unify_row(&a, &Row::pure()),
        Err(UnifyError::MissingEffects(vec![io]))
    );
}

#[test]
fn an_open_row_absorbs_the_missing_labels() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let r = table.fresh_row_var();
    let open = Row {
        labels: vec![],
        tail: Tail::Var(r),
    };
    assert_eq!(
        table.unify_row(&open, &Row::closed(vec![Label::plain(io)])),
        Ok(())
    );
    assert_eq!(
        table.resolve_row(&open),
        Row::closed(vec![Label::plain(io)])
    );
    let sigma = table.row_multiplicity_var(r);
    assert_eq!(
        solve(&table.multiplicity).0[sigma.index()],
        Multiplicity::Once
    );
}

#[test]
fn an_open_row_cannot_add_labels_to_a_closed_row() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let r = table.fresh_row_var();
    let open = Row {
        labels: vec![Label::plain(io)],
        tail: Tail::Var(r),
    };
    assert_eq!(
        table.unify_row(&open, &Row::pure()),
        Err(UnifyError::MissingEffects(vec![io]))
    );
}

#[test]
fn two_open_rows_share_a_fresh_tail() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let r1 = table.fresh_row_var();
    let r2 = table.fresh_row_var();
    let a = Row {
        labels: vec![Label::plain(io)],
        tail: Tail::Var(r1),
    };
    let b = Row {
        labels: vec![],
        tail: Tail::Var(r2),
    };
    assert_eq!(table.unify_row(&a, &b), Ok(()));
    let a = table.resolve_row(&a);
    let b = table.resolve_row(&b);
    assert_eq!(a.labels, vec![Label::plain(io)]);
    assert_eq!(b.labels, vec![Label::plain(io)]);
    assert_eq!(a.tail, b.tail);
}

#[test]
fn rows_with_the_same_tail_and_different_labels_report_the_missing_effects() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let r = table.fresh_row_var();
    let a = Row {
        labels: vec![Label::plain(io)],
        tail: Tail::Var(r),
    };
    let b = Row {
        labels: vec![],
        tail: Tail::Var(r),
    };
    assert_eq!(
        table.unify_row(&a, &b),
        Err(UnifyError::MissingEffects(vec![io]))
    );
}

#[test]
fn a_variable_unified_with_error_becomes_error() {
    let context = test_context();
    let mut table = Table::new(&context);
    let v = table.fresh_var();
    assert_eq!(table.unify(v, table.error), Ok(()));
    assert_eq!(table.unify(v, table.int), Ok(()));
    assert_eq!(table.unify(v, table.string), Ok(()));
}

#[test]
fn export_keeps_an_open_row() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let r = table.fresh_row_var();
    let f = table.function(
        table.int,
        Row {
            labels: vec![Label::plain(io)],
            tail: Tail::Var(r),
        },
        table.int,
    );
    assert_eq!(shown(&table, f), "Int -> <IO | _> Int");
    let v = table.fresh_var();
    assert_eq!(shown(&table, v), "_");
}

#[test]
fn rigid_variables_unify_only_with_themselves_and_flexible_variables() {
    let context = test_context();
    let mut table = Table::new(&context);
    let (a, _) = table.fresh_rigid("a");
    let (b, _) = table.fresh_rigid("b");
    assert_eq!(table.unify(a, a), Ok(()));
    assert_eq!(table.unify(a, b), Err(UnifyError::Mismatch));
    assert_eq!(table.unify(a, table.int), Err(UnifyError::Mismatch));
    let v = table.fresh_var();
    assert_eq!(table.unify(v, a), Ok(()));
    assert_eq!(shown(&table, v), "a");
}

#[test]
fn a_rigid_row_variable_cannot_be_bound() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let e = table.fresh_rigid_row("e");
    let rigid = Row {
        labels: vec![],
        tail: Tail::Var(e),
    };
    assert_eq!(
        table.unify_row(&rigid, &Row::closed(vec![Label::plain(io)])),
        Err(UnifyError::Mismatch)
    );
    let f = table.function(table.int, rigid.clone(), table.int);
    assert_eq!(shown(&table, f), "Int -> <e> Int");
}

#[test]
fn a_closed_callee_row_is_included_in_a_larger_row() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io_effect = table.context.externs.io;
    let io = Row::closed(vec![Label::plain(io_effect)]);
    assert_eq!(table.include_row(&Row::pure(), &io), Ok(Vec::new()));
    assert_eq!(table.include_row(&io, &io), Ok(Vec::new()));
    assert_eq!(
        table.include_row(&io, &Row::pure()),
        Err(UnifyError::MissingEffects(vec![io_effect]))
    );
}

#[test]
fn a_rigid_callee_row_needs_the_same_variable_in_the_ambient_row() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let e = table.fresh_rigid_row("e");
    let callee = Row {
        labels: vec![],
        tail: Tail::Var(e),
    };
    let wider = Row {
        labels: vec![Label::plain(io)],
        tail: Tail::Var(e),
    };
    assert_eq!(table.include_row(&callee, &wider), Ok(Vec::new()));
    assert_eq!(table.include_row(&callee, &callee.clone()), Ok(Vec::new()));
    assert_eq!(
        table.include_row(&callee, &Row::closed(vec![Label::plain(io)])),
        Err(UnifyError::MissingRowVar("e".to_string()))
    );
}

#[test]
fn open_spine_opens_only_the_rows_on_the_return_side() {
    let context = test_context();
    let mut table = Table::new(&context);
    let param = table.function(table.int, Row::pure(), table.int);
    let inner = table.function(table.int, Row::pure(), table.int);
    let f = table.function(param, Row::pure(), inner);
    let opened = table.open_spine(f);
    assert_eq!(shown(&table, opened), "(Int -> Int) -> <_> Int -> <_> Int");
}

#[test]
fn a_rigid_callee_row_extends_a_flexible_ambient_row() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let e = table.fresh_rigid_row("e");
    let fresh = table.fresh_row_var();
    let ambient = Row {
        labels: vec![Label::plain(io)],
        tail: Tail::Var(fresh),
    };
    let callee = Row {
        labels: vec![],
        tail: Tail::Var(e),
    };
    assert_eq!(table.include_row(&callee, &ambient), Ok(Vec::new()));
    let resolved = table.resolve_row(&ambient);
    assert_eq!(resolved.labels, vec![Label::plain(io)]);
    assert_eq!(resolved.tail, Tail::Var(e));
}

#[test]
fn binding_a_variable_passes_the_kind_of_its_type_to_the_variable() {
    let context = test_context();
    let mut table = Table::new(&context);
    let (a, ra) = table.fresh_rigid("a");
    let v = table.fresh_var();
    // v の Kind は Unr でなければならない。v を a に束縛すると、a の Kind も Unr になる
    table.kind_at_most(v, Bound::Const(Linearity::Unr));
    assert_eq!(table.unify(v, a), Ok(()));
    let mu = table.rigid_linearity(ra);
    assert_eq!(
        residual_of(&table.linearity, &[mu]),
        vec![(Bound::Var(mu), Bound::Const(Linearity::Unr))]
    );
}

#[test]
fn closure_kinds_bound_each_partial_application() {
    let context = test_context();
    let mut table = Table::new(&context);
    let (a, ra) = table.fresh_rigid("a");
    let m = table.fresh_arrow_lin();
    let inner = table.function_with(a, m, Row::pure(), a);
    let f = table.function(a, Row::pure(), inner);
    table.closure_kinds(f, 2, &[]);
    let ArrowLin::Var(m) = m else { unreachable!() };
    let mu = table.rigid_linearity(ra);
    assert_eq!(
        residual_of(&table.linearity, &[mu, m]),
        vec![(Bound::Var(mu), Bound::Var(m))]
    );
}

#[test]
fn an_error_row_unifies_with_any_row_without_binding() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let e = table.fresh_rigid_row("e");
    assert_eq!(
        table.unify_row(&Row::error(), &Row::closed(vec![Label::plain(io)])),
        Ok(())
    );
    let rigid = Row {
        labels: vec![Label::plain(io)],
        tail: Tail::Var(e),
    };
    assert_eq!(table.unify_row(&rigid, &Row::error()), Ok(()));
    assert_eq!(table.resolve_row(&rigid), rigid);
}

#[test]
fn a_flexible_row_unified_with_an_error_row_becomes_an_error_row() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let r = table.fresh_row_var();
    let open = Row {
        labels: vec![],
        tail: Tail::Var(r),
    };
    let error = Row {
        labels: vec![Label::plain(io)],
        tail: Tail::Error,
    };
    assert_eq!(table.unify_row(&open, &error), Ok(()));
    assert_eq!(table.resolve_row(&open), error);
}

#[test]
fn an_error_row_is_included_and_includes_any_row() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    assert_eq!(
        table.include_row(&Row::error(), &Row::pure()),
        Ok(Vec::new())
    );
    assert_eq!(
        table.include_row(&Row::closed(vec![Label::plain(io)]), &Row::error()),
        Ok(Vec::new())
    );
    let e = table.fresh_rigid_row("e");
    let rigid = Row {
        labels: vec![],
        tail: Tail::Var(e),
    };
    assert_eq!(table.include_row(&rigid, &Row::error()), Ok(Vec::new()));
}

#[test]
fn an_error_row_keeps_its_own_labels() {
    // 末尾 `Error` が受け入れるのは相手の側にしかないラベルだけである。自分の側の既知のラベルは、閉じた row に
    // 含まれなければ報告する。綴り誤りの E1002 と無関係なエフェクトの誤りを隠さないため
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let io_error = Row {
        labels: vec![Label::plain(io)],
        tail: Tail::Error,
    };
    assert_eq!(
        table.unify_row(&io_error, &Row::pure()),
        Err(UnifyError::MissingEffects(vec![io]))
    );
    assert_eq!(
        table.include_row(&io_error, &Row::pure()),
        Err(UnifyError::MissingEffects(vec![io]))
    );
    let e = table.fresh_rigid_row("e");
    let rigid = Row {
        labels: vec![],
        tail: Tail::Var(e),
    };
    assert_eq!(
        table.unify_row(&io_error, &rigid),
        Err(UnifyError::MissingEffects(vec![io]))
    );
}

#[test]
fn an_extern_effect_is_performed_at_most_once() {
    // 操作の多重度の最大を取ると、操作のない extern のエフェクトは `Never` になってしまう (docs/spec/types.md の「Kind」)
    let context = test_context();
    let table = Table::new(&context);
    assert_eq!(
        table.effect_multiplicity(table.context.externs.io),
        Multiplicity::Once
    );
}

#[test]
fn a_repeated_io_label_resolves_to_one() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = Label::plain(table.context.externs.io);
    let tail = table.fresh_row_var();
    let row = Row {
        labels: vec![io.clone()],
        tail: Tail::Var(tail),
    };
    assert_eq!(
        table.unify_row(
            &Row {
                labels: Vec::new(),
                tail: Tail::Var(tail)
            },
            &Row::closed(vec![io.clone()])
        ),
        Ok(())
    );
    assert_eq!(table.resolve_row(&row), Row::closed(vec![io]));
}

#[test]
fn a_repeated_io_label_keeps_the_other_labels_in_place() {
    let program = crate::test_program("effect Fail where\n  fail : Unit -> Unit");
    let context = Context::new(&program);
    let mut table = Table::new(&context);
    let io = Label::plain(table.context.externs.io);
    let fail = Label::plain(effect_named(&program, "Fail"));
    let tail = table.fresh_row_var();
    let row = Row {
        labels: vec![io.clone(), fail.clone()],
        tail: Tail::Var(tail),
    };
    assert_eq!(
        table.unify_row(
            &Row {
                labels: Vec::new(),
                tail: Tail::Var(tail)
            },
            &Row::closed(vec![io.clone()])
        ),
        Ok(())
    );
    assert_eq!(table.resolve_row(&row), Row::closed(vec![io, fail]));
}

#[test]
fn a_rigid_callee_row_masks_the_handleable_labels_before_its_variable() {
    let program = crate::test_program("effect State s where\n  get : Unit -> s");
    let context = Context::new(&program);
    let mut table = Table::new(&context);
    let state = effect_named(&program, "State");
    let label = Label {
        effect: state,
        args: vec![table.int],
    };
    let e = table.fresh_rigid_row("e");
    let callee = Row {
        labels: Vec::new(),
        tail: Tail::Var(e),
    };
    let ambient = Row {
        labels: vec![label, Label::plain(table.context.externs.io)],
        tail: Tail::Var(e),
    };
    assert_eq!(table.include_row(&callee, &ambient), Ok(vec![state]));
}

#[test]
fn a_mask_cannot_skip_an_effect_the_callee_names() {
    let program = crate::test_program("effect State s where\n  get : Unit -> s");
    let context = Context::new(&program);
    let mut table = Table::new(&context);
    let state = effect_named(&program, "State");
    let label = Label {
        effect: state,
        args: vec![table.int],
    };
    let e = table.fresh_rigid_row("e");
    let callee = Row {
        labels: vec![label.clone()],
        tail: Tail::Var(e),
    };
    let ambient = Row {
        labels: vec![label.clone(), label],
        tail: Tail::Var(e),
    };
    assert_eq!(
        table.include_row(&callee, &ambient),
        Err(UnifyError::MaskConflict(state))
    );
}

#[test]
fn labels_of_one_effect_pair_up_in_order() {
    let program = crate::test_program("effect State s where\n  get : Unit -> s");
    let context = Context::new(&program);
    let mut table = Table::new(&context);
    let state = effect_named(&program, "State");
    let label = |args: Vec<Ty>| Label {
        effect: state,
        args,
    };
    let a = Row::closed(vec![label(vec![table.int]), label(vec![table.string])]);
    let x = table.fresh_var();
    let y = table.fresh_var();
    let b = Row::closed(vec![label(vec![x]), label(vec![y])]);
    assert_eq!(table.unify_row(&a, &b), Ok(()));
    assert_eq!(shown(&table, x), "Int");
    assert_eq!(shown(&table, y), "String");
}

#[test]
fn labels_with_different_type_arguments_do_not_unify() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let left = Label {
        effect: io,
        args: vec![table.string],
    };
    let right = Label {
        effect: io,
        args: vec![table.int],
    };
    assert_eq!(
        table.unify_row(
            &Row::closed(vec![left.clone()]),
            &Row::closed(vec![right.clone()])
        ),
        Err(UnifyError::EffectArgs { left, right })
    );
}

#[test]
fn a_row_variable_cannot_occur_in_a_label_argument_of_its_row() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let r = table.fresh_row_var();
    let open = Row {
        labels: Vec::new(),
        tail: Tail::Var(r),
    };
    let int = table.int;
    let g = table.function(int, open.clone(), int);
    let with_g = Row {
        labels: vec![Label {
            effect: io,
            args: vec![g],
        }],
        tail: Tail::Var(table.fresh_row_var()),
    };
    assert_eq!(table.unify_row(&open, &with_g), Err(UnifyError::Occurs));
}

#[test]
fn an_infinite_label_argument_is_not_an_argument_mismatch() {
    let context = test_context();
    let mut table = Table::new(&context);
    let io = table.context.externs.io;
    let int = table.int;
    let x = table.fresh_var();
    let label = |args: Vec<Ty>| Label { effect: io, args };
    let f = table.function(int, Row::closed(vec![label(vec![x])]), int);
    assert_eq!(
        table.unify_row(
            &Row::closed(vec![label(vec![x])]),
            &Row::closed(vec![label(vec![f])])
        ),
        Err(UnifyError::Occurs)
    );
}

#[test]
fn children_of_arrows_are_the_parameter_the_row_and_the_result() {
    let context = test_context();
    let mut table = Table::new(&context);
    let int = table.int;
    let string = table.string;
    let function = table.function(int, Row::pure(), string);
    let mut seen = Vec::new();
    table.shape(function).for_each_child(|child| {
        seen.push(match child {
            Child::Ty(ty) if ty == int => "param",
            Child::Ty(ty) if ty == string => "ret",
            Child::Ty(_) => "other",
            Child::Row(_) => "row",
        })
    });
    assert_eq!(seen, ["param", "row", "ret"]);
}
