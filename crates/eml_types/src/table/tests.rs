use super::*;

#[test]
fn constructors_unify_only_with_themselves() {
    let mut table = Table::new();
    assert_eq!(table.unify(table.int, table.int), Ok(()));
    assert_eq!(
        table.unify(table.int, table.string),
        Err(UnifyError::Mismatch)
    );
    assert_eq!(table.unify(table.error, table.string), Ok(()));
}

#[test]
fn variables_are_bound_by_unification() {
    let mut table = Table::new();
    let v = table.fresh_var();
    let f = table.function(table.int, Row::pure(), v);
    let g = table.function(table.int, Row::pure(), table.bool);
    assert_eq!(table.unify(f, g), Ok(()));
    assert_eq!(table.display(v), Type::Bool);
}

#[test]
fn the_occurs_check_rejects_infinite_types() {
    let mut table = Table::new();
    let v = table.fresh_var();
    let f = table.function(v, Row::pure(), table.int);
    assert_eq!(table.unify(v, f), Err(UnifyError::Occurs));
}

#[test]
fn closed_rows_unify_regardless_of_order() {
    let mut table = Table::new();
    let a = Row::closed(vec![Effect::Io]);
    assert_eq!(table.unify_row(&a, &a.clone()), Ok(()));
    assert_eq!(
        table.unify_row(&a, &Row::pure()),
        Err(UnifyError::MissingEffects(vec![Effect::Io]))
    );
}

#[test]
fn an_open_row_absorbs_the_missing_labels() {
    let mut table = Table::new();
    let r = table.fresh_row_var();
    let open = Row {
        labels: vec![],
        tail: Tail::Var(r),
    };
    assert_eq!(
        table.unify_row(&open, &Row::closed(vec![Effect::Io])),
        Ok(())
    );
    assert_eq!(table.resolve_row(&open), Row::closed(vec![Effect::Io]));
    assert_eq!(table.row_multiplicity(r), Multiplicity::Once);
}

#[test]
fn an_open_row_cannot_add_labels_to_a_closed_row() {
    let mut table = Table::new();
    let r = table.fresh_row_var();
    let open = Row {
        labels: vec![Effect::Io],
        tail: Tail::Var(r),
    };
    assert_eq!(
        table.unify_row(&open, &Row::pure()),
        Err(UnifyError::MissingEffects(vec![Effect::Io]))
    );
}

#[test]
fn two_open_rows_share_a_fresh_tail() {
    let mut table = Table::new();
    let r1 = table.fresh_row_var();
    let r2 = table.fresh_row_var();
    let a = Row {
        labels: vec![Effect::Io],
        tail: Tail::Var(r1),
    };
    let b = Row {
        labels: vec![],
        tail: Tail::Var(r2),
    };
    assert_eq!(table.unify_row(&a, &b), Ok(()));
    let a = table.resolve_row(&a);
    let b = table.resolve_row(&b);
    assert_eq!(a.labels, vec![Effect::Io]);
    assert_eq!(b.labels, vec![Effect::Io]);
    assert_eq!(a.tail, b.tail);
}

#[test]
fn rows_with_the_same_tail_and_different_labels_report_the_missing_effects() {
    let mut table = Table::new();
    let r = table.fresh_row_var();
    let a = Row {
        labels: vec![Effect::Io],
        tail: Tail::Var(r),
    };
    let b = Row {
        labels: vec![],
        tail: Tail::Var(r),
    };
    assert_eq!(
        table.unify_row(&a, &b),
        Err(UnifyError::MissingEffects(vec![Effect::Io]))
    );
}

#[test]
fn a_variable_unified_with_error_becomes_error() {
    let mut table = Table::new();
    let v = table.fresh_var();
    assert_eq!(table.unify(v, table.error), Ok(()));
    assert_eq!(table.unify(v, table.int), Ok(()));
    assert_eq!(table.unify(v, table.string), Ok(()));
}

#[test]
fn export_keeps_an_open_row() {
    let mut table = Table::new();
    let r = table.fresh_row_var();
    let f = table.function(
        table.int,
        Row {
            labels: vec![Effect::Io],
            tail: Tail::Var(r),
        },
        table.int,
    );
    assert_eq!(table.display(f).to_string(), "Int -> <IO | _> Int");
    let v = table.fresh_var();
    assert_eq!(table.display(v).to_string(), "_");
}

#[test]
fn rigid_variables_unify_only_with_themselves_and_flexible_variables() {
    let mut table = Table::new();
    let (a, _) = table.fresh_rigid("a");
    let (b, _) = table.fresh_rigid("b");
    assert_eq!(table.unify(a, a), Ok(()));
    assert_eq!(table.unify(a, b), Err(UnifyError::Mismatch));
    assert_eq!(table.unify(a, table.int), Err(UnifyError::Mismatch));
    let v = table.fresh_var();
    assert_eq!(table.unify(v, a), Ok(()));
    assert_eq!(table.display(v).to_string(), "a");
}

#[test]
fn a_rigid_row_variable_cannot_be_bound() {
    let mut table = Table::new();
    let e = table.fresh_rigid_row("e");
    let rigid = Row {
        labels: vec![],
        tail: Tail::Var(e),
    };
    assert_eq!(
        table.unify_row(&rigid, &Row::closed(vec![Effect::Io])),
        Err(UnifyError::Mismatch)
    );
    let f = table.function(table.int, rigid.clone(), table.int);
    assert_eq!(table.display(f).to_string(), "Int -> <e> Int");
}

#[test]
fn a_closed_callee_row_is_included_in_a_larger_row() {
    let mut table = Table::new();
    let io = Row::closed(vec![Effect::Io]);
    assert_eq!(table.include_row(&Row::pure(), &io), Ok(()));
    assert_eq!(table.include_row(&io, &io), Ok(()));
    assert_eq!(
        table.include_row(&io, &Row::pure()),
        Err(UnifyError::MissingEffects(vec![Effect::Io]))
    );
}

#[test]
fn a_rigid_callee_row_needs_the_same_variable_in_the_ambient_row() {
    let mut table = Table::new();
    let e = table.fresh_rigid_row("e");
    let callee = Row {
        labels: vec![],
        tail: Tail::Var(e),
    };
    let wider = Row {
        labels: vec![Effect::Io],
        tail: Tail::Var(e),
    };
    assert_eq!(table.include_row(&callee, &wider), Ok(()));
    assert_eq!(table.include_row(&callee, &callee.clone()), Ok(()));
    assert_eq!(
        table.include_row(&callee, &Row::closed(vec![Effect::Io])),
        Err(UnifyError::MissingRowVar("e".to_string()))
    );
}

#[test]
fn open_spine_opens_only_the_rows_on_the_return_side() {
    let mut table = Table::new();
    let param = table.function(table.int, Row::pure(), table.int);
    let inner = table.function(table.int, Row::pure(), table.int);
    let f = table.function(param, Row::pure(), inner);
    let opened = table.open_spine(f);
    assert_eq!(
        table.display(opened).to_string(),
        "(Int -> Int) -> <_> Int -> <_> Int"
    );
}

#[test]
fn copy_type_replaces_rigid_variables() {
    let mut table = Table::new();
    let (a, ra) = table.fresh_rigid("a");
    let e = table.fresh_rigid_row("e");
    let f = table.function(
        a,
        Row {
            labels: vec![],
            tail: Tail::Var(e),
        },
        a,
    );
    let mut subst = Subst::default();
    subst.tys.insert(ra, table.int);
    let copied = table.copy_type(f, &subst);
    assert_eq!(table.display(copied).to_string(), "Int -> <e> Int");
    let mut subst = Subst::default();
    subst.rows.insert(e, table.fresh_row_var());
    let copied = table.copy_type(f, &subst);
    assert_eq!(table.display(copied).to_string(), "a -> <_> a");
}

#[test]
fn a_rigid_callee_row_extends_a_flexible_ambient_row() {
    let mut table = Table::new();
    let e = table.fresh_rigid_row("e");
    let fresh = table.fresh_row_var();
    let ambient = Row {
        labels: vec![Effect::Io],
        tail: Tail::Var(fresh),
    };
    let callee = Row {
        labels: vec![],
        tail: Tail::Var(e),
    };
    assert_eq!(table.include_row(&callee, &ambient), Ok(()));
    let resolved = table.resolve_row(&ambient);
    assert_eq!(resolved.labels, vec![Effect::Io]);
    assert_eq!(resolved.tail, Tail::Var(e));
}

#[test]
fn binding_a_variable_passes_the_kind_of_its_type_to_the_variable() {
    let mut table = Table::new();
    let (a, ra) = table.fresh_rigid("a");
    let v = table.fresh_var();
    // v の Kind は Unr でなければならない。v を a に束縛すると、a の Kind も Unr になる
    table.kind_at_most(v, Bound::Const(Linearity::Unr));
    assert_eq!(table.unify(v, a), Ok(()));
    let mu = table.rigid_linearity(ra);
    assert_eq!(
        table.lin_residual(&[mu]),
        vec![(Bound::Var(mu), Bound::Const(Linearity::Unr))]
    );
}

#[test]
fn closure_kinds_bound_each_partial_application() {
    let mut table = Table::new();
    let (a, ra) = table.fresh_rigid("a");
    let m = table.fresh_arrow_lin();
    let inner = table.function_with(a, m, Row::pure(), a);
    let f = table.function(a, Row::pure(), inner);
    table.closure_kinds(f, 2, &[]);
    let ArrowLin::Var(m) = m else { unreachable!() };
    let mu = table.rigid_linearity(ra);
    assert_eq!(
        table.lin_residual(&[mu, m]),
        vec![(Bound::Var(mu), Bound::Var(m))]
    );
}

#[test]
#[should_panic(expected = "export is for after solve_kinds")]
fn export_needs_solved_kinds() {
    let table = Table::new();
    table.export(table.int);
}

#[test]
fn display_does_not_solve_kinds() {
    let mut table = Table::new();
    let lin = table.fresh_arrow_lin();
    let f = table.function_with(table.int, lin, Row::pure(), table.int);
    assert_eq!(table.display(f).to_string(), "Int -> Int");
}

#[test]
fn an_error_row_unifies_with_any_row_without_binding() {
    let mut table = Table::new();
    let e = table.fresh_rigid_row("e");
    assert_eq!(
        table.unify_row(&Row::error(), &Row::closed(vec![Effect::Io])),
        Ok(())
    );
    let rigid = Row {
        labels: vec![Effect::Io],
        tail: Tail::Var(e),
    };
    assert_eq!(table.unify_row(&rigid, &Row::error()), Ok(()));
    assert_eq!(table.resolve_row(&rigid), rigid);
}

#[test]
fn a_flexible_row_unified_with_an_error_row_becomes_an_error_row() {
    let mut table = Table::new();
    let r = table.fresh_row_var();
    let open = Row {
        labels: vec![],
        tail: Tail::Var(r),
    };
    let error = Row {
        labels: vec![Effect::Io],
        tail: Tail::Error,
    };
    assert_eq!(table.unify_row(&open, &error), Ok(()));
    assert_eq!(table.resolve_row(&open), error);
}

#[test]
fn an_error_row_is_included_and_includes_any_row() {
    let mut table = Table::new();
    assert_eq!(table.include_row(&Row::error(), &Row::pure()), Ok(()));
    assert_eq!(
        table.include_row(&Row::closed(vec![Effect::Io]), &Row::error()),
        Ok(())
    );
    let e = table.fresh_rigid_row("e");
    let rigid = Row {
        labels: vec![],
        tail: Tail::Var(e),
    };
    assert_eq!(table.include_row(&rigid, &Row::error()), Ok(()));
}

#[test]
fn copying_keeps_an_error_row() {
    let mut table = Table::new();
    let f = table.function(table.int, Row::error(), table.int);
    let copied = table.copy_type(f, &Subst::default());
    assert_eq!(table.display(copied).to_string(), "Int -> <{error}> Int");
}

#[test]
fn an_error_row_keeps_its_own_labels() {
    // 末尾 `Error` が受け入れるのは相手の側にしかないラベルだけである。自分の側の既知のラベルは、閉じた row に
    // 含まれなければ報告する。綴り誤りの E1002 と無関係なエフェクトの誤りを隠さないため
    let mut table = Table::new();
    let io_error = Row {
        labels: vec![Effect::Io],
        tail: Tail::Error,
    };
    assert_eq!(
        table.unify_row(&io_error, &Row::pure()),
        Err(UnifyError::MissingEffects(vec![Effect::Io]))
    );
    assert_eq!(
        table.include_row(&io_error, &Row::pure()),
        Err(UnifyError::MissingEffects(vec![Effect::Io]))
    );
    let e = table.fresh_rigid_row("e");
    let rigid = Row {
        labels: vec![],
        tail: Tail::Var(e),
    };
    assert_eq!(
        table.unify_row(&io_error, &rigid),
        Err(UnifyError::MissingEffects(vec![Effect::Io]))
    );
}
