//! Core IR のテキストで書いた IR で、verifier が正しいものを受け入れ、壊れたものを拒むことを確かめる (docs/spec/core-ir.md)。

use eml_core_ir::{verify, verify_scopes};

fn check(text: &str) -> Result<(), String> {
    let program = eml_core_ir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    verify(&program).map_err(|error| error.to_string())
}

fn check_scopes(text: &str) -> Result<(), String> {
    let program = eml_core_ir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    verify_scopes(&program).map_err(|error| error.to_string())
}

/// `pick b s = let t = if b then s else "none" in t ++ s` の Perceus の後の形。
/// `dup_before_jump` が偽のとき、then 枝は `s` を複製せずに渡す。
fn pick(dup_before_jump: bool) -> String {
    let dup = if dup_before_jump {
        "      dup s1\n"
    } else {
        ""
    };
    format!(
        "fn pick(b0, s1^) {{
  join j0(t3^) [s1] {{
    let t4^ = prim ++(t3, s1)
    return t4
  }}
  switch b0 {{
    #0 ->
      let s2^ = const \"s\"
      jump j0(s2)
    #1 ->
{dup}      jump j0(s1)
  }}
}}
"
    )
}

/// `two s = let u = "s" in join j0(a, b) [] { let c = a ++ b; return c } in jump j0(s, u)` の Perceus の後の形。
/// `passed` を変えて、`jump` が渡す値の数を変える。
fn two_values(passed: &str) -> String {
    format!(
        "fn two(s0^) {{
  join j0(a2^, b3^) [] {{
    let c4^ = prim ++(a2, b3)
    return c4
  }}
  let u1^ = const \"s\"
  jump j0({passed})
}}
"
    )
}

/// `g s = s`。
const IDENTITY: &str = "fn g(s0^) {\n  return s0\n}\n";

/// 両方の枝が scrutinee の `d` を返す。`Switch` は `d` を move で受け取るので、枝で使うには前で複製する。
fn keep_scrutinee(dup: bool) -> String {
    let dup = if dup { "  dup d0\n" } else { "" };
    format!(
        "fn f(d0^) {{
{dup}  switch d0 {{
    #0 ->
      return d0
    #1(x1^) ->
      decref x1
      return d0
  }}
}}
"
    )
}

/// `release` は、フィールドを持つ枝が `x` を解放するかどうか。
fn unused_field(release: bool) -> String {
    let decref = if release { "      decref x1\n" } else { "" };
    format!(
        "fn f(d0^) {{
  switch d0 {{
    #0 ->
      return ()
    #1(x1^) ->
{decref}      return ()
  }}
}}
"
    )
}

const ASK: &str = "effect Ask { ask/1 }\n";

/// `handle ask 1 with | ask x k -> resume k x` を持ち上げた形。`effects` は先頭のエフェクトの行。
fn handler_program(effects: &str) -> String {
    format!(
        "{effects}fn main() {{
  let c0^ = &main$handle0
  let c1^ = &main$handle0$ask
  let t2 = handle Ask(c0, ()) {{ask: c1}} return &main$handle0$return
  return t2
}}
fn main$handle0(p0) {{
  tailcall perform Ask.ask(1)
}}
fn main$handle0$ask(x0, k1^, s2) {{
  tailcall resume k1(x0, s2)
}}
fn main$handle0$return(x0, s1) {{
  return x0
}}
"
    )
}

#[test]
fn a_duplicated_string_used_twice_is_accepted() {
    let text = r#"fn twice(s0^) {
  dup s0
  let t1^ = prim ++(s0, s0)
  return t1
}
"#;
    assert_eq!(check(text), Ok(()));
}

#[test]
fn a_join_point_is_accepted() {
    assert_eq!(check(&pick(true)), Ok(()));
}

#[test]
fn a_tail_call_that_takes_every_owned_value_is_accepted() {
    let text = r#"fn id(s0^) {
  return s0
}
fn caller(s0^) {
  tailcall id(s0)
}
"#;
    assert_eq!(check(text), Ok(()));
}

#[test]
fn a_variable_bound_twice_is_rejected() {
    let text = r#"fn f() {
  let s0^ = const "s"
  let s0^ = const "s"
  return s0
}
"#;
    assert_eq!(check(text), Err("`s0` is bound twice in `f`".to_string()));
}

#[test]
fn a_variable_used_outside_its_scope_is_rejected() {
    let text = r#"fn f() {
  return s0
}
"#;
    assert_eq!(
        check(text),
        Err("`s0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_jump_outside_its_join_scope_is_rejected() {
    let text = r#"fn f() {
  join j0(t0) [] {
    jump j0(1)
  }
  jump j0(2)
}
"#;
    assert_eq!(
        check(text),
        Err("a jump to `j0` is outside its scope in `f`".to_string())
    );
}

#[test]
fn a_use_after_a_move_is_rejected() {
    let text = r#"fn twice(s0^) {
  let t1^ = prim ++(s0, s0)
  return t1
}
"#;
    assert_eq!(
        check(text),
        Err("`s0` is used after it was moved in `twice`".to_string())
    );
}

#[test]
fn a_missing_decref_is_rejected() {
    let text = r#"fn ignore(s0^) {
  return 1
}
"#;
    assert_eq!(
        check(text),
        Err("`s0` is still owned at the end of the function in `ignore`".to_string())
    );
}

#[test]
fn a_double_decref_is_rejected() {
    let text = r#"fn ignore(s0^) {
  decref s0
  decref s0
  return 1
}
"#;
    assert_eq!(
        check(text),
        Err("`s0` is released after it was moved in `ignore`".to_string())
    );
}

#[test]
fn a_jump_that_owns_too_much_is_rejected() {
    let text = r#"fn f(s0^) {
  join j0(t1) [] {
    return t1
  }
  jump j0(1)
}
"#;
    assert_eq!(
        check(text),
        Err("a jump to `j0` owns [s0] but its join needs [] in `f`".to_string())
    );
}

#[test]
fn a_jump_that_owns_too_little_is_rejected() {
    assert_eq!(
        check(&pick(false)),
        Err("a jump to `j0` owns [] but its join needs [s1] in `pick`".to_string())
    );
}

#[test]
fn a_join_point_with_two_parameters_is_accepted() {
    // 本体は2つの引数をどちらも所有して始まり、`jump` は渡す値の所有権を渡す
    assert_eq!(check(&two_values("s0, u1")), Ok(()));
}

#[test]
fn a_jump_with_the_wrong_number_of_values_is_rejected() {
    assert_eq!(
        check(&two_values("s0")),
        Err("a jump to `j0` passes 1 values, but its join takes 2 in `two`".to_string())
    );
}

#[test]
fn a_direct_call_with_the_wrong_number_of_arguments_is_rejected() {
    let text = r#"fn g(a0) {
  return a0
}
fn f() {
  let t0 = call g(1, 2)
  return t0
}
"#;
    assert_eq!(
        check(text),
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

#[test]
fn a_call_that_does_not_save_an_owned_variable_is_rejected() {
    // f s = dup s; let t = g s; s を退避しないまま t ++ s
    let text = format!(
        "{IDENTITY}fn f(s0^) {{
  dup s0
  let t1^ = call g(s0)
  let t2^ = prim ++(t1, s0)
  return t2
}}
"
    );
    assert_eq!(
        check(&text),
        Err("a call saves [] but owns [s0] in `f`".to_string())
    );
}

#[test]
fn a_call_that_saves_a_variable_it_does_not_own_is_rejected() {
    let text = format!(
        "{IDENTITY}fn f(s0^) {{
  let t1^ = call g(s0) [s0]
  return t1
}}
"
    );
    assert_eq!(
        check(&text),
        Err("a call saves [s0] but owns [] in `f`".to_string())
    );
}

#[test]
fn a_variable_not_saved_by_a_call_is_out_of_scope_after_it() {
    let text = r#"fn k(a0) {
  return a0
}
fn f(n0) {
  let t1 = call k(n0)
  let t2 = prim +(n0, t1)
  return t2
}
"#;
    assert_eq!(
        check(text),
        Err("`n0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_jump_after_a_call_needs_the_variables_of_the_join_body_in_scope() {
    let text = r#"fn z() {
  return 1
}
fn f(n0) {
  join j0(t1) [n0] {
    let t2 = prim +(n0, t1)
    return t2
  }
  let t3 = call z()
  jump j0(t3)
}
"#;
    assert_eq!(
        check(text),
        Err("`n0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_call_that_saves_a_variable_twice_is_rejected() {
    // 退避する変数が重なると、フレームが所有していない参照を、解放や複製のときに数えてしまう
    let text = format!(
        "{IDENTITY}fn f(s0^) {{
  dup s0
  let t1^ = call g(s0) [s0, s0]
  let t2^ = prim ++(t1, s0)
  return t2
}}
"
    );
    assert_eq!(
        check(&text),
        Err("a call saves [s0, s0] but owns [s0] in `f`".to_string())
    );
}

#[test]
fn handlers_operations_and_resume_are_calls() {
    assert_eq!(check(&handler_program(ASK)), Ok(()));
}

#[test]
fn a_handler_has_a_clause_for_each_operation() {
    let text = handler_program("effect Ask { ask/1, tell/1 }\n");
    assert_eq!(
        check(&text),
        Err(
            "a handler of `Ask` has clauses for 1 operations, but the effect has 2 in `main`"
                .to_string()
        )
    );
}

#[test]
fn a_clause_receives_the_arguments_k_and_the_state_after_its_captures() {
    let text = "effect Ask { ask/1 }\nfn f() {\n  let t0 = handle Ask(&body, ()) {ask: &clause} return &ret\n  return t0\n}\nfn body(u0) {\n  return 1\n}\nfn clause(x0, k1^) {\n  tailcall resume k1(x0, ())\n}\nfn ret(x0, s1) {\n  return x0\n}\n";
    let error = check(text).unwrap_err();
    assert!(
        error.contains("the clause for `ask` needs 3 parameters after its captures (1 for the arguments, `k`, and the state), but it has 2"),
        "{error}"
    );
    // 節の引数の数は所有権に関わらないので、Perceus より前の IR でも確かめる
    let error = check_scopes(text).unwrap_err();
    assert!(
        error.contains("the clause for `ask` needs 3 parameters after its captures (1 for the arguments, `k`, and the state), but it has 2"),
        "{error}"
    );
}

#[test]
fn a_clause_of_a_never_operation_receives_the_arguments_and_the_state() {
    let text = "effect Fail { never fail/1 }\nfn f(n0) {\n  let c1^ = closure clause(n0)\n  let t2 = handle Fail(&body, ()) {fail: c1} return &ret\n  return t2\n}\nfn body(u0) {\n  return 1\n}\nfn clause(n0, x1, k2, s3) {\n  return n0\n}\nfn ret(x0, s1) {\n  return x0\n}\n";
    let error = check(text).unwrap_err();
    assert!(
        error.contains("the clause for `fail` needs 2 parameters after its captures (1 for the arguments and the state), but it has 3"),
        "{error}"
    );
}

#[test]
fn a_return_clause_receives_the_value_and_the_state_after_its_captures() {
    let text = "effect Ask { ask/1 }\nfn f(n0) {\n  let c1^ = closure ret(n0)\n  let t2 = handle Ask(&body, ()) {ask: &clause} return c1\n  return t2\n}\nfn body(u0) {\n  return 1\n}\nfn clause(x0, k1^, s2) {\n  tailcall resume k1(x0, s2)\n}\nfn ret(n0, x1) {\n  return x1\n}\n";
    let error = check(text).unwrap_err();
    assert!(
        error.contains("the `return` clause needs 2 parameters after its captures (the value and the state), but it has 1"),
        "{error}"
    );
}

#[test]
fn perform_names_an_operation_of_its_effect() {
    let text = format!("{ASK}fn f() {{\n  tailcall perform Ask.#1()\n}}\n");
    assert_eq!(
        check(&text),
        Err("`perform` names operation 1 of `Ask`, which has 1 operations in `f`".to_string())
    );
}

#[test]
fn drop_takes_the_ownership_of_its_value() {
    let once = "fn f(s0^) {\n  let t1 = drop s0\n  return t1\n}\n";
    assert_eq!(check(once), Ok(()));
    let twice = "fn g(s0^) {\n  let t1 = drop s0\n  let u2 = drop s0\n  return u2\n}\n";
    assert_eq!(
        check(twice),
        Err("`s0` is used after it was moved in `g`".to_string())
    );
}

#[test]
fn a_join_body_that_uses_a_variable_missing_from_its_captures_is_rejected() {
    // f n = join j0(t) [] { let u = n + t; return u } in jump j0(1)
    let text = r#"fn f(n0) {
  join j0(t1) [] {
    let u2 = prim +(n0, t1)
    return u2
  }
  jump j0(1)
}
"#;
    assert_eq!(
        check(text),
        Err("`n0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_capture_out_of_scope_at_its_join_is_rejected() {
    // f n = join j0(t) [x] { return t } in jump j0(n)。`x` はどこでも束縛していない
    let text = r#"fn f(n0) {
  join j0(t1) [x2] {
    return t1
  }
  jump j0(n0)
}
"#;
    assert_eq!(
        check(text),
        Err("`j0` captures `x2`, which is not in scope in `f`".to_string())
    );
}

#[test]
fn captures_out_of_order_are_rejected() {
    let text = r#"fn f(a0, b1) {
  join j0(t2) [b1, a0] {
    return t2
  }
  jump j0(1)
}
"#;
    assert_eq!(
        check(text),
        Err("the captures of `j0` are not in increasing order in `f`".to_string())
    );
}

#[test]
fn an_unused_capture_released_by_the_body_is_accepted() {
    // f s = join j0(t) [s] { decref s; return t } in jump j0(1)。`captures` は最小でなくてよい
    let text = r#"fn f(s0^) {
  join j0(t1) [s0] {
    decref s0
    return t1
  }
  jump j0(1)
}
"#;
    assert_eq!(check(text), Ok(()));
}

#[test]
fn scopes_accept_a_value_used_twice_without_dup() {
    // Perceus より前の IR には `dup` がないので、所有は数えない
    let text = r#"fn twice(s0^) {
  let t1^ = prim ++(s0, s0)
  return t1
}
"#;
    assert_eq!(check_scopes(text), Ok(()));
}

#[test]
fn scopes_accept_a_join_point_before_perceus() {
    assert_eq!(check_scopes(&pick(false)), Ok(()));
}

#[test]
fn scopes_keep_variables_in_scope_after_a_call() {
    // `saved` は Perceus が決めるので、その前は呼び出しの後で範囲を区切り直さない
    let text = r#"fn k(a0) {
  return a0
}
fn f(n0) {
  let t1 = call k(n0)
  let t2 = prim +(n0, t1)
  return t2
}
"#;
    assert_eq!(check_scopes(text), Ok(()));
}

#[test]
fn scopes_reject_a_dup() {
    let text = r#"fn twice(s0^) {
  dup s0
  let t1^ = prim ++(s0, s0)
  return t1
}
"#;
    assert_eq!(
        check_scopes(text),
        Err("`s0` is duplicated before Perceus in `twice`".to_string())
    );
}

#[test]
fn scopes_reject_a_decref() {
    let text = r#"fn ignore(s0^) {
  decref s0
  return 1
}
"#;
    assert_eq!(
        check_scopes(text),
        Err("`s0` is released before Perceus in `ignore`".to_string())
    );
}

#[test]
fn scopes_reject_a_saved_list() {
    let text = format!(
        "{IDENTITY}fn f(s0^) {{
  let t1^ = call g(s0) [s0]
  return t1
}}
"
    );
    assert_eq!(
        check_scopes(&text),
        Err("a call saves [s0] before Perceus in `f`".to_string())
    );
}

#[test]
fn scopes_reject_a_variable_used_outside_its_scope() {
    let text = r#"fn f() {
  return s0
}
"#;
    assert_eq!(
        check_scopes(text),
        Err("`s0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_switch_that_binds_fields_is_accepted() {
    let text = r#"fn f(d0^) {
  switch d0 {
    #0 ->
      return ()
    #1(x1^) ->
      return x1
  }
}
"#;
    assert_eq!(check(text), Ok(()));
}

#[test]
fn an_unused_field_must_be_released() {
    assert_eq!(check(&unused_field(true)), Ok(()));
    assert_eq!(
        check(&unused_field(false)),
        Err("`x1` is still owned at the end of the function in `f`".to_string())
    );
}

#[test]
fn a_scrutinee_used_in_an_arm_is_duplicated_before_the_switch() {
    assert_eq!(check(&keep_scrutinee(true)), Ok(()));
    assert_eq!(
        check(&keep_scrutinee(false)),
        Err("`d0` is used after it was moved in `f`".to_string())
    );
}

#[test]
fn a_field_is_in_scope_only_in_its_arm() {
    let text = r#"fn f(d0^) {
  switch d0 {
    #0 ->
      return x1
    #1(x1^) ->
      return x1
  }
}
"#;
    assert_eq!(
        check_scopes(text),
        Err("`x1` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_switch_on_an_unboxed_variable_cannot_bind_fields() {
    let text = r#"fn f(d0) {
  switch d0 {
    #0 ->
      return ()
    #1(x1^) ->
      return x1
  }
}
"#;
    assert_eq!(
        check_scopes(text),
        Err("`d0` is not boxed, but a switch binds its fields in `f`".to_string())
    );
}

#[test]
fn a_closure_without_arguments_is_rejected() {
    let text = "fn f() {\n  let c0^ = closure g()\n  return c0\n}\nfn g(x0) {\n  return x0\n}\n";
    let error = check(text).unwrap_err();
    assert!(
        error.contains("a closure of `g` has no arguments; use `&g`"),
        "{error}"
    );
}

#[test]
fn a_literal_switch_with_a_default_is_accepted() {
    let text = "fn main(n0) {
  switch n0 {
    1 ->
      return 10
    2 ->
      return 20
    _ ->
      return 0
  }
}
";
    assert_eq!(check(text), Ok(()));
}

#[test]
fn a_literal_switch_without_a_default_is_rejected() {
    let text = "fn main(n0) {
  switch n0 {
    1 ->
      return 10
  }
}
";
    let error = check(text).unwrap_err();
    assert!(error.contains("default"), "{error}");
}

#[test]
fn a_switch_that_mixes_kinds_of_cases_is_rejected() {
    let text = "fn main(n0) {
  switch n0 {
    #0 ->
      return 10
    1 ->
      return 20
    _ ->
      return 0
  }
}
";
    let error = check(text).unwrap_err();
    assert!(error.contains("mixes"), "{error}");
}

#[test]
fn a_switch_with_two_cases_for_one_literal_is_rejected() {
    let text = "fn main(n0) {
  switch n0 {
    1 ->
      return 10
    1 ->
      return 20
    _ ->
      return 0
  }
}
";
    let error = check(text).unwrap_err();
    assert!(error.contains("two cases"), "{error}");
}
