//! `var` の文、代入、添字の HIR (docs/spec/expressions.md の「`var`」)。

use eml_hir::{Body, EffectKind, ExprId, ExprKind, IndexBase, LocalId, Program, Var, VarId};

use crate::common::{diagnostics, lower_text};

fn body<'m>(program: &'m Program, name: &str) -> &'m Body {
    program
        .functions()
        .find(|(_, function)| function.name == name)
        .and_then(|(id, _)| program.body(id))
        .expect("the body")
}

fn var_node(body: &Body, name: &str) -> (ExprId, Option<ExprId>, ExprId) {
    body.exprs
        .iter()
        .find_map(|(id, expr)| match &expr.kind {
            ExprKind::Var {
                var,
                body: inner,
                last,
                ..
            } if body.vars[*var].name == name => Some((id, *last, *inner)),
            _ => None,
        })
        .expect("the var node")
}

fn var_named<'b>(body: &'b Body, name: &str) -> (VarId, &'b Var) {
    body.vars
        .iter()
        .find(|(_, var)| var.name == name)
        .expect("the var")
}

#[test]
fn a_var_scopes_the_rest_of_the_block() {
    let text = "g : Int -> Unit\ng _ = ()\n\nf : Unit -> Int\nf () =\n  g 0\n  var x = 1\n  g x\n  x <- 2\n  x";
    insta::assert_snapshot!(lower_text(text), @"
    var effect var$0$x = x#v0 in f
    g : Int -> Unit
    g _ = ()
    f : Unit -> Int
    f () = {
      (@g 0)
      (var x#v0 = 1 in {
        (@g x#v0)
        (x#v0 <- 2)
        x#v0
      })
    }
    ");
    let lowered = eml_test_support::lower(text);
    let body = body(&lowered.program, "f");
    let (_, last, inner) = var_node(body, "x");
    let ExprKind::Block { stmts, tail, .. } = &body.exprs[inner].kind else {
        panic!("the body of a var is a block");
    };
    assert_eq!(stmts.len(), 2);
    assert_eq!(*tail, last);
    let (x, _) = var_named(body, "x");
    assert_eq!(
        last.map(|last| &body.exprs[last].kind),
        Some(&ExprKind::VarGet(x))
    );
}

#[test]
fn a_var_at_the_end_of_a_block_has_no_last_expression() {
    let text = "f : Unit -> Unit\nf () =\n  var x = 1\n  let y = 2\n  var z = y\n\ng : Unit -> Unit\ng () =\n  var w = 1";
    insta::assert_snapshot!(lower_text(text), @"
    var effect var$0$x = x#v0 in f
    var effect var$1$z = z#v1 in f
    var effect var$2$w = w#v0 in g
    f : Unit -> Unit
    f () = {
      (var x#v0 = 1 in {
        let y#0 = 2
        (var z#v1 = y#0 in {
        })
      })
    }
    g : Unit -> Unit
    g () = {
      (var w#v0 = 1 in {
      })
    }
    ");
    let lowered = eml_test_support::lower(text);
    let f = body(&lowered.program, "f");
    assert_eq!(var_node(f, "x").1, None);
    assert_eq!(var_node(f, "z").1, None);
    let g = body(&lowered.program, "g");
    let (_, last, inner) = var_node(g, "w");
    assert_eq!(last, None);
    assert!(matches!(
        &g.exprs[inner].kind,
        ExprKind::Block { stmts, tail: None, .. } if stmts.is_empty()
    ));
}

#[test]
fn a_var_makes_one_effect_per_site() {
    let text = "f : Unit -> Int\nf () =\n  var x = 1\n  x\n\ng : Unit -> Int\ng () =\n  var x = 2\n  x\n\nh : Unit -> Int\nh () =\n  var x = 3\n  var y = 4\n  x";
    insta::assert_snapshot!(lower_text(text), @"
    var effect var$0$x = x#v0 in f
    var effect var$1$x = x#v0 in g
    var effect var$2$x = x#v0 in h
    var effect var$3$y = y#v1 in h
    f : Unit -> Int
    f () = {
      (var x#v0 = 1 in {
        x#v0
      })
    }
    g : Unit -> Int
    g () = {
      (var x#v0 = 2 in {
        x#v0
      })
    }
    h : Unit -> Int
    h () = {
      (var x#v0 = 3 in {
        (var y#v1 = 4 in {
          x#v0
        })
      })
    }
    ");
    let lowered = eml_test_support::lower(text);
    let program = &lowered.program;
    let effects: Vec<(String, String, String, String)> = program
        .effects()
        .filter(|(id, _)| id.module == program.entry)
        .map(|(id, effect)| {
            let EffectKind::Var { function, var } = effect.kind else {
                panic!("a synthetic effect");
            };
            let body = program.body(function).expect("the body");
            assert_eq!(body.vars[var].effect, id);
            assert!(effect.operations.is_empty());
            assert!(effect.generics.type_vars.is_empty());
            (
                effect.name.clone(),
                program.names.effect(id).to_string(),
                program[function].name.clone(),
                body.vars[var].name.clone(),
            )
        })
        .collect();
    let expected = [
        ("var$0$x", "var x", "f", "x"),
        ("var$1$x", "var x", "g", "x"),
        ("var$2$x", "var x", "h", "x"),
        ("var$3$y", "var y", "h", "y"),
    ];
    assert_eq!(
        effects,
        expected.map(|(a, b, c, d)| (a.into(), b.into(), c.into(), d.into()))
    );
}

#[test]
fn uses_lower_to_var_nodes() {
    let text = "f : Int -> Int\nf i =\n  var a = i\n  var n : Int = 0\n  n <- a[i]\n  a[i] <- n\n  let g = fn () -> n\n  let n = 5\n  n";
    insta::assert_snapshot!(lower_text(text), @"
    var effect var$0$a = a#v0 in f
    var effect var$1$n = n#v1 in f
    f : Int -> Int
    f i#0 = {
      (var a#v0 = i#0 in {
        (var n#v1 : Int = 0 in {
          (n#v1 <- a#v0[i#0])
          (a#v0[i#0] <- n#v1)
          let g#1 = (fn () -> n#v1)
          let n#2 = 5
          n#2
        })
      })
    }
    ");
}

#[test]
fn a_var_init_sees_the_outer_binding() {
    let text = "f : Int -> Int\nf x =\n  var x = x\n  x";
    insta::assert_snapshot!(lower_text(text), @"
    var effect var$0$x = x#v0 in f
    f : Int -> Int
    f x#0 = {
      (var x#v0 = x#0 in {
        x#v0
      })
    }
    ");
}

#[test]
fn a_non_var_assignment_target_is_e1053() {
    let text = "f : Int -> Int\nf x =\n  let y = 1\n  y <- 1\n  f x <- 1\n  y[0] <- 2\n  x";
    insta::assert_snapshot!(diagnostics(text).join("\n"), @"
    E1053 4:3 only a `var` variable or an index of one can be assigned
    E1053 5:3 only a `var` variable or an index of one can be assigned
    E1053 6:3 only a `var` variable or an index of one can be assigned
    ");
}

#[test]
fn an_index_of_an_expression_has_an_expr_base() {
    let text = "f : Int -> Int\nf x =\n  let ys = x\n  (f x)[0] + ys[1]";
    insta::assert_snapshot!(lower_text(text), @"
    f : Int -> Int
    f x#0 = {
      let ys#1 = x#0
      (@Prelude.+ (@f x#0)[0] ys#1[1])
    }
    ");
    let lowered = eml_test_support::lower(text);
    let body = body(&lowered.program, "f");
    let bases: Vec<bool> = body
        .exprs
        .iter()
        .filter_map(|(_, expr)| match &expr.kind {
            ExprKind::Index { base, .. } => Some(matches!(base, IndexBase::Expr(_))),
            _ => None,
        })
        .collect();
    assert_eq!(bases, [true, true]);
}

#[test]
fn lambdas_do_not_capture_vars() {
    let text = "f : Int -> Int\nf k =\n  var x = 1\n  let g = fn () -> x + k\n  g ()";
    let lowered = eml_test_support::lower(text);
    let body = body(&lowered.program, "f");
    let closure = body
        .exprs
        .iter()
        .find_map(|(_, expr)| match &expr.kind {
            ExprKind::Lambda(closure) => Some(closure),
            _ => None,
        })
        .expect("the lambda");
    let names: Vec<&str> = body
        .closure_captures(closure)
        .iter()
        .map(|&local| body.locals[local].name.as_str())
        .collect();
    assert_eq!(names, ["k"]);
}

#[test]
fn outer_bindings_are_recorded() {
    let text = "f : Int -> Int\nf k =\n  var x = 1\n  let a = 2\n  let g = fn y ->\n    var z = y\n    z\n  g a";
    let lowered = eml_test_support::lower(text);
    let body = body(&lowered.program, "f");
    let locals = |locals: &[LocalId]| -> Vec<String> {
        locals
            .iter()
            .map(|&local| body.locals[local].name.clone())
            .collect()
    };
    let vars = |vars: &[VarId]| -> Vec<String> {
        vars.iter()
            .map(|&var| body.vars[var].name.clone())
            .collect()
    };
    let (_, x) = var_named(body, "x");
    assert_eq!(locals(&x.outer_locals), ["k"]);
    assert!(x.outer_vars.is_empty());
    let (_, z) = var_named(body, "z");
    assert_eq!(locals(&z.outer_locals), ["k", "a", "y"]);
    assert_eq!(vars(&z.outer_vars), ["x"]);
}

/// 名前のない `var` (E0011) の後ろの文も、ふつうの文として下ろす。
#[test]
fn a_var_without_a_name_keeps_the_rest_of_the_block() {
    let text = "f : Unit -> Int\nf () =\n  var (a, b) = 1\n  let c = 2\n  c";
    insta::assert_snapshot!(lower_text(text), @"
    f : Unit -> Int
    f () = {
      <missing>
      let c#0 = 2
      c#0
    }
    ---
    E0011 3:7 expected a variable name
    ");
}
