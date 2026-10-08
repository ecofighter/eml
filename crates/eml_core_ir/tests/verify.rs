//! Core IR のテキストで書いた IR で、verifier が正しいものを受け入れ、壊れたものを拒むことを確かめる (docs/spec/core-ir.md)。
//! 構造の規則 R1 から R8 と、前の形の IR から引き継いだ検査を1つずつ確かめる。

use eml_core_ir::{Program, Stmt, Term, parse, verify, verify_scopes};

/// 手で書く IR の配置の行。プログラムの先頭に置く。
const BOOL: &str = "layout Prelude.Bool { False, True }\n";
const OPTION: &str = "layout Option { None, Some(tobj) }\n";
const PAIR: &str = "layout (,) { (,)(tobj, tobj) }\n";
const BOX: &str = "layout Box { Box(tobj) }\n";

fn read(text: &str) -> Program {
    parse(text).unwrap_or_else(|error| panic!("{error}"))
}

fn check(text: &str) -> Result<(), String> {
    verify(&read(text)).map_err(|error| error.to_string())
}

fn check_scopes(text: &str) -> Result<(), String> {
    verify_scopes(&read(text)).map_err(|error| error.to_string())
}

/// spec の「テキストの形」の例。RC の対象の変数がないので、どちらの段でも通る。
const SPEC_EXAMPLE: &str = "\
layout Prelude.Bool { False, True }
fn f(x.0: int) -> int {
  let c.1: enum = extern Prelude.<(x.0, 10)
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  jump b3(x.0)
b2:
  let t.2: int = extern Prelude.+(x.0, 1) @\"main.em\":2:20
  jump b3(t.2)
b3(t.3: int):
  return t.3
}
";

/// `pick b s = let t = if b then "s" else s in t ++ s` の Perceus の後の形。`dup` が偽のとき、b2 は `s` を複製せずに
/// 渡すので、b3 へ入る2本の `jump` の所有がそろわない。
fn pick(dup: bool) -> String {
    let dup = if dup { "  dup s.1\n" } else { "" };
    format!(
        "{BOOL}fn pick(b.0: enum, s.1: obj) -> obj {{
  switch b.0 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let s.2: obj = const \"s\"
  jump b3(s.2)
b2:
{dup}  jump b3(s.1)
b3(t.3: obj):
  let t.4: obj = extern Prelude.++(t.3, s.1)
  return t.4
}}
"
    )
}

/// `g s = s`。
const IDENTITY: &str = "fn g(s.0: obj) -> obj {\n  return s.0\n}\n";

/// `k n = n`。
const K: &str = "fn k(a.0: int) -> int {\n  return a.0\n}\n";

/// `d` を `switch` で分解し、`#1` の行き先 b2 の `body` が、`d` から借りたフィールド `x` を使う。`#0` の行き先は
/// 所有をすべて手放す。`c` は `apply` で呼ぶ値である。
fn borrowing_arm(body: &str) -> String {
    format!(
        "{OPTION}{BOX}{IDENTITY}{K}fn f(d.0: tobj, c.1: tobj) -> obj {{
  switch d.0 Option {{ #0 -> b1, #1(x.2: obj) -> b2 }}
b1:
  decref d.0
  decref c.1
  let e.3: obj = const \"e\"
  return e.3
b2:
{body}}}
"
    )
}

/// `p` を `unpack` で分解し、`body` が `p` から借りたフィールド `a` と `b` を使う。
fn unpacking(body: &str) -> String {
    format!(
        "{PAIR}{OPTION}{BOX}fn f(p.0: obj) -> obj {{\n  unpack p.0 (,) #0(a.1: obj, b.2: tobj)\n{body}}}\n"
    )
}

/// `n` を使う合流のブロックの前で、片方の経路だけが呼び出しをする。`saved` はその呼び出しの `save` の部分である。
/// `call_first` が偽なら、呼び出しのない経路を先のブロックに置く。合流のブロックの区間が、先に着いた `jump` の区間
/// で決まらないことを確かめるためである。
fn call_on_one_path(saved: &str, call_first: bool) -> String {
    let call = format!("  let t.2: int = call k(1){saved}\n  jump b3(t.2)\n");
    let other = "  jump b3(1)\n";
    let (first, second) = if call_first {
        (call.as_str(), other)
    } else {
        (other, call.as_str())
    };
    format!(
        "{BOOL}{K}fn f(n.0: int, c.1: enum) -> int {{
  switch c.1 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}
b1:
{first}b2:
{second}b3(r.3: int):
  let u.4: int = extern Prelude.+(n.0, r.3)
  return u.4
}}
"
    )
}

const ASK: &str = "effect Ask { ask/1 }\n";

/// `handle ask 1 with | ask x k -> resume k x` を持ち上げた形。`effects` は先頭のエフェクトの行。
fn handler_program(effects: &str) -> String {
    format!(
        "{effects}fn main() -> int {{
  let c.0: tobj = closure main$handle0(1)
  let c.1: tobj = closure main$handle0$ask(2)
  let t.2: int = handle Ask((), c.0) {{ ask: c.1 }} return &main$handle0$return
  return t.2
}}
fn main$handle0(n.0: int, p.1: unit) -> int {{
  tail perform Ask.ask(n.0)
}}
fn main$handle0$ask(m.0: int, x.1: int, k.2: tobj, s.3: unit) -> int {{
  tail resume k.2(x.1, s.3)
}}
fn main$handle0$return(x.0: int, s.1: unit) -> int {{
  return x.0
}}
"
    )
}

#[test]
fn the_spec_example_is_accepted() {
    assert_eq!(check_scopes(SPEC_EXAMPLE), Ok(()));
    assert_eq!(check(SPEC_EXAMPLE), Ok(()));
}

#[test]
fn a_duplicated_string_used_twice_is_accepted() {
    let text = "\
fn twice(s.0: obj) -> obj {
  dup s.0
  let t.1: obj = extern Prelude.++(s.0, s.0)
  return t.1
}
";
    assert_eq!(check(text), Ok(()));
}

#[test]
fn a_tail_call_that_takes_every_owned_value_is_accepted() {
    let text = format!("{IDENTITY}fn caller(s.0: obj) -> obj {{\n  tail call g(s.0)\n}}\n");
    assert_eq!(check(&text), Ok(()));
}

// R1

#[test]
fn an_edge_to_the_entry_block_is_rejected() {
    let text = "fn f(x.0: int) -> int {\n  jump b1()\nb1:\n  jump b0(x.0)\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("an edge from b1 goes to the entry block in `f`".to_string())
    );
}

// R2

#[test]
fn an_edge_back_to_an_earlier_block_is_rejected() {
    let text = "fn f() -> int {\n  jump b1()\nb1:\n  jump b2()\nb2:\n  jump b1()\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("an edge from b2 goes back to b1 in `f`".to_string())
    );
}

#[test]
fn an_edge_to_itself_is_rejected() {
    let text = "fn f() -> int {\n  jump b1()\nb1:\n  jump b1()\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("an edge from b1 goes back to b1 in `f`".to_string())
    );
}

#[test]
fn an_edge_to_a_missing_block_is_rejected() {
    let mut program = read("fn f() -> int {\n  jump b1()\nb1:\n  return 1\n}\n");
    program.functions[0].blocks.pop();
    assert_eq!(
        verify_scopes(&program).map_err(|error| error.to_string()),
        Err("an edge from b0 goes to b1, which does not exist in `f`".to_string())
    );
}

// R3

#[test]
fn a_switch_target_entered_by_a_jump_is_rejected() {
    let text = "\
fn f(x.0: int) -> int {
  switch x.0 { 1 -> b1, _ -> b2 }
b1:
  jump b2()
b2:
  return x.0
}
";
    assert_eq!(
        check_scopes(text),
        Err("b2 is the target of both a switch and a jump in `f`".to_string())
    );
}

#[test]
fn a_switch_target_with_parameters_is_rejected() {
    let text = "\
fn f(x.0: int) -> int {
  switch x.0 { 1 -> b1, _ -> b2 }
b1:
  return 1
b2(y.1: int):
  return y.1
}
";
    assert_eq!(
        check_scopes(text),
        Err("b2 is the target of a switch but takes parameters in `f`".to_string())
    );
}

#[test]
fn a_block_entered_by_two_switch_edges_is_rejected() {
    let text = "\
fn f(x.0: int) -> int {
  switch x.0 { 1 -> b1, _ -> b1 }
b1:
  return x.0
}
";
    assert_eq!(
        check_scopes(text),
        Err("b1 is the target of 2 switch edges in `f`".to_string())
    );
}

// R4

#[test]
fn a_block_without_an_edge_into_it_is_rejected() {
    let text = "fn f() -> int {\n  return 1\nb1:\n  return 2\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("b1 has no edge into it in `f`".to_string())
    );
}

#[test]
fn a_jump_with_the_wrong_number_of_values_is_rejected() {
    let text = "fn f() -> int {\n  jump b1(1, 2)\nb1(y.0: int):\n  return y.0\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("a jump to b1 passes 2 values, but b1 takes 1 in `f`".to_string())
    );
}

#[test]
fn a_merge_block_with_two_parameters_is_accepted() {
    // 合流のブロックは、RC の対象の引数をどちらも所有して始まり、`jump` は渡す値の所有を渡す
    let text = "\
fn two(s.0: obj) -> obj {
  let u.1: obj = const \"s\"
  jump b1(s.0, u.1)
b1(a.2: obj, b.3: obj):
  let c.4: obj = extern Prelude.++(a.2, b.3)
  return c.4
}
";
    assert_eq!(check(text), Ok(()));
}

// R5

#[test]
fn a_variable_defined_twice_is_rejected() {
    let text = "\
fn f() -> obj {
  let s.0: obj = const \"s\"
  let s.0: obj = const \"s\"
  return s.0
}
";
    assert_eq!(
        check(text),
        Err("`s.0` is defined twice in `f`".to_string())
    );
}

#[test]
fn a_variable_defined_in_two_blocks_is_rejected() {
    // 両方の経路で定義しても、合流のブロックでは使えない。値は引数で渡す
    let text = "\
fn f(x.0: int) -> int {
  switch x.0 { 1 -> b1, _ -> b2 }
b1:
  let t.1: int = extern Prelude.+(x.0, 1)
  jump b3()
b2:
  let t.1: int = extern Prelude.+(x.0, 2)
  jump b3()
b3:
  return t.1
}
";
    assert_eq!(
        check_scopes(text),
        Err("`t.1` is defined twice in `f`".to_string())
    );
}

#[test]
fn a_field_and_a_parameter_with_one_number_are_rejected() {
    let text = "\
layout Option { None, Some(tobj) }
fn f(d.0: tobj) -> unit {
  switch d.0 Option { #0 -> b1, #1(d.0: tobj) -> b2 }
b1:
  return ()
b2:
  return ()
}
";
    assert_eq!(
        check_scopes(text),
        Err("`d.0` is defined twice in `f`".to_string())
    );
}

// R6: 支配

#[test]
fn a_variable_used_outside_its_scope_is_rejected() {
    let text = "fn f() -> obj {\n  return s.0\n}\n";
    assert_eq!(
        check(text),
        Err("`s.0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_variable_used_before_its_definition_is_rejected() {
    let text = "\
fn f(x.0: int) -> int {
  let a.1: int = extern Prelude.+(b.2, 1)
  let b.2: int = extern Prelude.+(x.0, 1)
  return a.1
}
";
    assert_eq!(
        check_scopes(text),
        Err("`b.2` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_variable_used_in_its_own_definition_is_rejected() {
    let text = "fn f() -> int {\n  let a.0: int = extern Prelude.+(a.0, 1)\n  return a.0\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("`a.0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_variable_of_another_branch_is_rejected() {
    let text = "\
fn f(x.0: int) -> int {
  switch x.0 { 1 -> b1, _ -> b2 }
b1:
  let t.1: int = extern Prelude.+(x.0, 1)
  jump b3(t.1)
b2:
  jump b3(t.1)
b3(r.2: int):
  return r.2
}
";
    assert_eq!(
        check_scopes(text),
        Err("`t.1` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_merge_block_does_not_see_a_variable_defined_on_one_path() {
    let text = "\
fn f(x.0: int) -> int {
  switch x.0 { 1 -> b1, _ -> b2 }
b1:
  let t.1: int = extern Prelude.+(x.0, 1)
  jump b3()
b2:
  jump b3()
b3:
  return t.1
}
";
    assert_eq!(
        check_scopes(text),
        Err("`t.1` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_field_is_in_scope_only_in_its_arm() {
    let text = "\
layout Option { None, Some(tobj) }
fn f(d.0: tobj) -> obj {
  switch d.0 Option { #0 -> b1, #1(x.1: obj) -> b2 }
b1:
  return x.1
b2:
  return x.1
}
";
    assert_eq!(
        check_scopes(text),
        Err("`x.1` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_field_is_in_scope_in_the_blocks_its_arm_dominates() {
    let text = "\
layout Prelude.Bool { False, True }
layout Option { None, Some(tobj) }
fn f(d.0: tobj, c.1: enum) -> obj {
  switch d.0 Option { #0 -> b1, #1(x.2: obj) -> b2 }
b1:
  decref d.0
  let e.3: obj = const \"e\"
  return e.3
b2:
  switch c.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
b3:
  jump b5()
b4:
  jump b5()
b5:
  release d.0 Option #1(x.2)
  return x.2
}
";
    assert_eq!(check(text), Ok(()));
}

// R6: 所有

#[test]
fn a_merge_block_is_accepted() {
    assert_eq!(check(&pick(true)), Ok(()));
}

#[test]
fn jumps_that_own_different_values_are_rejected() {
    assert_eq!(
        check(&pick(false)),
        Err("a jump to b3 owns [] but an earlier jump to it owns [s.1] in `pick`".to_string())
    );
}

#[test]
fn scopes_accept_a_merge_block_before_perceus() {
    assert_eq!(check_scopes(&pick(false)), Ok(()));
}

#[test]
fn a_value_owned_by_every_jump_is_owned_by_the_merge_block() {
    let released = "\
layout Prelude.Bool { False, True }
fn f(s.0: obj, c.1: enum) -> int {
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  jump b3(1)
b2:
  jump b3(2)
b3(n.2: int):
  decref s.0
  return n.2
}
";
    assert_eq!(check(released), Ok(()));
    let kept = released.replace("  decref s.0\n", "");
    assert_eq!(
        check(&kept),
        Err("`s.0` is still owned at the end of the function in `f`".to_string())
    );
}

// R7

#[test]
fn a_call_that_does_not_save_an_owned_variable_is_rejected() {
    let text = format!(
        "{IDENTITY}fn f(s.0: obj) -> obj {{
  dup s.0
  let t.1: obj = call g(s.0)
  let t.2: obj = extern Prelude.++(t.1, s.0)
  return t.2
}}
"
    );
    assert_eq!(
        check(&text),
        Err("a call saves [] but owns [s.0] in `f`".to_string())
    );
}

#[test]
fn a_call_that_saves_a_variable_it_does_not_own_is_rejected() {
    let text = format!(
        "{IDENTITY}fn f(s.0: obj) -> obj {{\n  let t.1: obj = call g(s.0) save [s.0]\n  return t.1\n}}\n"
    );
    assert_eq!(
        check(&text),
        Err("a call saves [s.0] but owns [] in `f`".to_string())
    );
}

#[test]
fn a_call_that_saves_a_variable_twice_is_rejected() {
    // 退避する変数が重なると、フレームが所有していない参照を、解放や複製のときに数えてしまう
    let text = format!(
        "{IDENTITY}fn f(s.0: obj) -> obj {{
  dup s.0
  let t.1: obj = call g(s.0) save [s.0, s.0]
  let t.2: obj = extern Prelude.++(t.1, s.0)
  return t.2
}}
"
    );
    assert_eq!(
        check(&text),
        Err("a call saves [s.0, s.0] but owns [s.0] in `f`".to_string())
    );
}

#[test]
fn a_call_that_saves_an_invisible_variable_is_rejected() {
    let text =
        format!("{K}fn f() -> int {{\n  let t.0: int = call k(1) save [n.1]\n  return t.0\n}}\n");
    assert_eq!(
        check(&text),
        Err("`n.1` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_non_rc_variable_not_saved_by_a_call_is_not_visible_after_it() {
    let text = format!(
        "{K}fn f(n.0: int) -> int {{
  let t.1: int = call k(n.0)
  let t.2: int = extern Prelude.+(n.0, t.1)
  return t.2
}}
"
    );
    assert_eq!(
        check(&text),
        Err("`n.0` is used after a call that does not save it in `f`".to_string())
    );
    let saved = text.replace("call k(n.0)", "call k(n.0) save [n.0]");
    assert_eq!(check(&saved), Ok(()));
}

#[test]
fn a_merge_block_does_not_see_a_variable_that_one_path_did_not_save() {
    for call_first in [true, false] {
        assert_eq!(
            check(&call_on_one_path("", call_first)),
            Err("`n.0` is used after a call that does not save it in `f`".to_string())
        );
        assert_eq!(check(&call_on_one_path(" save [n.0]", call_first)), Ok(()));
    }
}

#[test]
fn scopes_keep_variables_in_scope_after_a_call() {
    // `saved` は Perceus が決めるので、その前は呼び出しの後で見える変数を区切り直さない
    let text = format!(
        "{K}fn f(n.0: int) -> int {{
  let t.1: int = call k(n.0)
  let t.2: int = extern Prelude.+(n.0, t.1)
  return t.2
}}
"
    );
    assert_eq!(check_scopes(&text), Ok(()));
    assert_eq!(check_scopes(&call_on_one_path("", true)), Ok(()));
}

#[test]
fn scopes_reject_a_saved_list() {
    let text = format!(
        "{IDENTITY}fn f(s.0: obj) -> obj {{\n  let t.1: obj = call g(s.0) save [s.0]\n  return t.1\n}}\n"
    );
    assert_eq!(
        check_scopes(&text),
        Err("a call saves [s.0] before Perceus in `f`".to_string())
    );
}

// R8

#[test]
fn a_jump_with_a_variable_of_another_repr_is_rejected() {
    let text = "fn f(x.0: int) -> obj {\n  jump b1(x.0)\nb1(s.1: obj):\n  return s.1\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("a jump to b1 passes `x.0` (int) to `s.1` (obj) in `f`".to_string())
    );
}

#[test]
fn a_jump_with_a_constant_that_does_not_fit_is_rejected() {
    let text = "fn f() -> int {\n  jump b1(#1)\nb1(y.0: int):\n  return y.0\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("a jump to b1 passes #1 to `y.0` (int) in `f`".to_string())
    );
}

#[test]
fn constants_fit_the_reprs_of_their_values() {
    // タグは `enum` にも、即値を持てる `tobj` にも入る。関数の値は `tobj` の即値である
    let text = "\
fn f() -> unit {
  jump b1(3, #1, #0, &f, ())
b1(n.0: int, e.1: enum, t.2: tobj, g.3: tobj, u.4: unit):
  decref t.2
  decref g.3
  return u.4
}
";
    assert_eq!(check(text), Ok(()));
}

#[test]
fn an_unpack_of_a_value_that_is_not_obj_is_rejected() {
    let text = "layout Box { Box(tobj) }\nfn f(p.0: tobj) -> int {\n  unpack p.0 Box #0(a.1: int)\n  return a.1\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("`p.0` (tobj) is unpacked, but only obj can be in `f`".to_string())
    );
}

#[test]
fn an_unpack_without_fields_is_rejected() {
    let text = "layout Box { Box(tobj) }\nfn f(p.0: obj) -> unit {\n  unpack p.0 Box #0()\n  return ()\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("an unpack of `p.0` binds no fields in `f`".to_string())
    );
}

#[test]
fn a_returned_variable_has_the_repr_of_the_function() {
    let text = "fn f(s.0: obj) -> int {\n  return s.0\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("`s.0` (obj) is returned from a function that returns int in `f`".to_string())
    );
}

#[test]
fn the_result_of_a_call_is_not_compared_with_the_callee() {
    // 呼び出しの結果と呼ばれる関数の `ret` は、S3b-2c まで比べない (docs/spec/core-ir.md の「構造の規則」)
    let text = format!("{K}fn f() -> obj {{\n  let t.0: obj = call k(1)\n  return t.0\n}}\n");
    assert_eq!(check_scopes(&text), Ok(()));
}

/// 所有を見る前に断る IR の誤り。どちらの段でも同じ誤りになる。
fn rejected_at_both_levels(text: &str) -> String {
    let error = check_scopes(text).expect_err(text);
    assert_eq!(check(text), Err(error.clone()));
    error
}

#[test]
fn an_extern_argument_of_another_repr_is_rejected() {
    let text =
        "fn f(s.0: obj) -> int {\n  let t.1: int = extern Prelude.+(s.0, 1)\n  return t.1\n}\n";
    assert_eq!(
        rejected_at_both_levels(text),
        "argument 0 of `Prelude.+` is `s.0` (obj), but the extern takes int in `f`"
    );
}

#[test]
fn an_extern_argument_constant_that_does_not_fit_is_rejected() {
    let text =
        "fn f(n.0: int) -> int {\n  let t.1: int = extern Prelude.+(n.0, ())\n  return t.1\n}\n";
    assert_eq!(
        rejected_at_both_levels(text),
        "argument 1 of `Prelude.+` is (), but the extern takes int in `f`"
    );
}

#[test]
fn an_extern_result_bound_to_another_repr_is_rejected() {
    let text =
        "fn f(n.0: int) -> obj {\n  let t.1: obj = extern Prelude.<(n.0, 1)\n  return t.1\n}\n";
    assert_eq!(
        rejected_at_both_levels(text),
        "`t.1` (obj) is bound to `Prelude.<`, which returns enum in `f`"
    );
}

#[test]
fn an_extern_takes_constants_that_fit_its_row() {
    // 引数のないコンストラクタは `enum` の引数に収まる
    let text =
        "fn f() -> enum {\n  let c.0: enum = extern Prelude.bool_eq(#1, #0)\n  return c.0\n}\n";
    assert_eq!(check_scopes(text), Ok(()));
    assert_eq!(check(text), Ok(()));
}

// 借りたフィールド

#[test]
fn a_borrowed_field_cannot_be_consumed() {
    let borrowed = "`x.2` is used but is only borrowed from `d.0` in `f`";
    for body in [
        "  return x.2\n",
        "  let t.4: obj = call g(x.2) save [d.0, c.1]\n  return t.4\n",
        "  let t.4: obj = con Box #0(x.2)\n  return t.4\n",
        "  let t.4: obj = extern Prelude.++(x.2, x.2)\n  return t.4\n",
        "  let t.4: obj = apply c.1(x.2)\n  return t.4\n",
        "  let t.4: obj = apply x.2(1)\n  return t.4\n",
    ] {
        assert_eq!(
            check(&borrowing_arm(body)),
            Err(borrowed.to_string()),
            "{body}"
        );
    }
    assert_eq!(
        check(&borrowing_arm("  decref x.2\n  return x.2\n")),
        Err("`x.2` is released but is only borrowed from `d.0` in `f`".to_string())
    );
}

#[test]
fn a_borrowed_field_cannot_be_passed_to_a_block() {
    let text = "\
layout Option { None, Some(tobj) }
fn f(d.0: tobj) -> obj {
  switch d.0 Option { #0 -> b1, #1(x.1: obj) -> b2 }
b1:
  decref d.0
  let e.2: obj = const \"e\"
  jump b3(e.2)
b2:
  jump b3(x.1)
b3(r.3: obj):
  return r.3
}
";
    assert_eq!(
        check(text),
        Err("`x.1` is used but is only borrowed from `d.0` in `f`".to_string())
    );
}

#[test]
fn a_borrowed_field_cannot_be_saved() {
    assert_eq!(
        check(&borrowing_arm(
            "  let t.4: int = call k(1) save [d.0, c.1, x.2]\n  return x.2\n"
        )),
        Err("a call saves [d.0, c.1, x.2] but owns [d.0, c.1] in `f`".to_string())
    );
}

#[test]
fn a_field_cannot_be_read_after_its_owner_is_given_up() {
    let given_up = |field: &str, what: &str| {
        Err(format!(
            "`{field}` is {what} after its owner `p.0` was given up in `f`"
        ))
    };
    // `release` が残さなかったフィールド
    assert_eq!(
        check(&unpacking(
            "  release p.0 (,) #0(a.1, _)\n  dup b.2\n  decref b.2\n  return a.1\n"
        )),
        given_up("b.2", "duplicated")
    );
    assert_eq!(
        check(&unpacking(
            "  release p.0 (,) #0(a.1, _)\n  switch b.2 Option { #0 -> b1, #1(z.3: obj) -> b2 }\nb1:\n  return a.1\nb2:\n  return a.1\n"
        )),
        given_up("b.2", "switched on")
    );
    // `decref` と消費で手放した持ち主
    assert_eq!(
        check(&unpacking("  decref p.0\n  dup a.1\n  return a.1\n")),
        given_up("a.1", "duplicated")
    );
    assert_eq!(
        check(&unpacking(
            "  let t.3: obj = con Box #0(p.0)\n  dup a.1\n  decref t.3\n  return a.1\n"
        )),
        given_up("a.1", "duplicated")
    );
    assert_eq!(
        check(&unpacking(
            "  decref p.0\n  unpack a.1 Box #0(z.3: obj)\n  return z.3\n"
        )),
        given_up("a.1", "unpacked")
    );
}

#[test]
fn a_field_consumed_after_its_owner_is_given_up_was_moved() {
    // 持ち主も手放した後に借りた変数を消費するか手放すと、移動の後の使用として報告する
    // (docs/spec/core-ir.md の「verifier」)
    assert_eq!(
        check(&unpacking("  decref p.0\n  return a.1\n")),
        Err("`a.1` is used after it was moved in `f`".to_string())
    );
    assert_eq!(
        check(&unpacking(
            "  release p.0 (,) #0(a.1, _)\n  decref b.2\n  return a.1\n"
        )),
        Err("`b.2` is released after it was moved in `f`".to_string())
    );
}

#[test]
fn a_release_keeps_only_the_fields_of_its_value() {
    let not_field = |field: &str, slot: usize, value: &str, tag: u32| {
        Err(format!(
            "`{field}` is not field {slot} of `{value}` #{tag} in `f`"
        ))
    };
    // 孫は `xs` のフィールドではない
    let grandchild = "\
layout Box { Box(tobj) }
fn f(xs.0: obj) -> obj {
  unpack xs.0 Box #0(y.1: obj)
  unpack y.1 Box #0(z.2: obj)
  release xs.0 Box #0(z.2)
  return z.2
}
";
    assert_eq!(check(grandchild), not_field("z.2", 0, "xs.0", 0));
    // 2回目の `unpack` の位置 0 のフィールドを、位置 1 に書く
    assert_eq!(
        check(&unpacking(
            "  unpack p.0 (,) #0(c.3: obj, d.4: tobj)\n  release p.0 (,) #0(a.1, c.3)\n  return a.1\n"
        )),
        not_field("c.3", 1, "p.0", 0)
    );
    assert_eq!(
        check(&unpacking("  release p.0 (,) #1(a.1, _)\n  return a.1\n")),
        not_field("a.1", 0, "p.0", 1)
    );
    assert_eq!(
        check(&unpacking("  release p.0 (,) #0(a.1)\n  return a.1\n")),
        not_field("a.1", 0, "p.0", 0)
    );
    let other = "\
layout Box { Box(tobj) }
fn f(p.0: obj, q.1: obj) -> obj {
  unpack p.0 Box #0(a.2: obj)
  unpack q.1 Box #0(b.3: obj)
  release p.0 Box #0(b.3)
  decref q.1
  return b.3
}
";
    assert_eq!(check(other), not_field("b.3", 0, "p.0", 0));
}

#[test]
fn a_release_cannot_keep_a_field_of_a_branch_that_does_not_dominate_it() {
    let text = "\
layout Option { None, Some(tobj) }
fn f(d.0: tobj) -> obj {
  switch d.0 Option { #0 -> b1, #1(x.1: obj) -> b2 }
b1:
  jump b3()
b2:
  jump b3()
b3:
  release d.0 Option #1(x.1)
  return x.1
}
";
    assert_eq!(
        check(text),
        Err("`x.1` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_value_is_released_once() {
    assert_eq!(
        check(&unpacking(
            "  release p.0 (,) #0(a.1, _)\n  release p.0 (,) #0(a.1, _)\n  return a.1\n"
        )),
        Err("`p.0` is released after it was moved in `f`".to_string())
    );
}

#[test]
fn a_borrowed_value_cannot_be_released() {
    let text = "\
layout Box { Box(tobj) }
fn f(d.0: obj) -> obj {
  unpack d.0 Box #0(y.1: obj)
  unpack y.1 Box #0(z.2: obj)
  release y.1 Box #0(z.2)
  decref d.0
  return z.2
}
";
    assert_eq!(
        check(text),
        Err("`y.1` is released but is only borrowed from `d.0` in `f`".to_string())
    );
}

#[test]
fn a_nested_release_is_accepted() {
    let text = "\
layout (,) { (,)(tobj, tobj) }
layout Box { Box(tobj) }
fn f(xs.0: obj) -> obj {
  unpack xs.0 (,) #0(y.1: obj, w.2: obj)
  release xs.0 (,) #0(y.1, _)
  unpack y.1 Box #0(z.3: obj)
  release y.1 Box #0(z.3)
  return z.3
}
";
    assert_eq!(check(text), Ok(()));
}

#[test]
fn a_field_duplicated_before_its_owner_is_given_up_stays_owned() {
    assert_eq!(
        check(&unpacking("  dup a.1\n  decref p.0\n  return a.1\n")),
        Ok(())
    );
}

#[test]
fn a_borrowed_field_can_be_switched_on_while_its_owner_is_owned() {
    let text = "\
layout Box { Box(tobj) }
layout Option { None, Some(tobj) }
fn f(d.0: obj) -> obj {
  unpack d.0 Box #0(y.1: tobj)
  switch y.1 Option { #0 -> b1, #1(z.2: obj) -> b2 }
b1:
  decref d.0
  let e.3: obj = const \"e\"
  return e.3
b2:
  dup z.2
  decref d.0
  return z.2
}
";
    assert_eq!(check(text), Ok(()));
}

#[test]
fn a_value_owned_twice_is_still_owned_after_a_release() {
    // `release` が手放すのは参照1つなので、`p` は所有されたままで、残さなかった `b` も有効である
    assert_eq!(
        check(&unpacking(
            "  dup p.0\n  release p.0 (,) #0(a.1, _)\n  dup b.2\n  decref p.0\n  decref b.2\n  return a.1\n"
        )),
        Ok(())
    );
}

#[test]
fn what_perceus_gives_a_target_that_uses_its_scrutinee_is_accepted() {
    // `label` はフィールドのない行き先と、フィールドを使う行き先の両方で、scrutinee を後でも使う。`rest` は
    // `| Nil -> xs` の形で、フィールドのない行き先が scrutinee をそのまま返す
    let text = "\
data Option a =
  | None
  | Some a

data List a =
  | Nil
  | Cons a (List a)

show : Option String -> String
show o = match o with
  | Some s -> s
  | None -> \"none\"

label : Option String -> String
label o =
  let first = match o with
    | Some s -> s ++ show o
    | None -> show o
  first ++ show o

rest : List Int -> List Int
rest xs = match xs with
  | Nil -> xs
  | Cons _ t -> t

size : List Int -> Int
size xs = match xs with
  | Nil -> 0
  | Cons _ t -> 1 + size t

main : Unit -> <IO> Unit
main () =
  println (label (Some \"a\"))
  println (show_int (size (rest (Cons 1 Nil))))
";
    assert_eq!(verify(&eml_test_support::core(text)), Ok(()));
}

// 所有の数え方

#[test]
fn a_use_after_a_move_is_rejected() {
    let text = "\
fn twice(s.0: obj) -> obj {
  let t.1: obj = extern Prelude.++(s.0, s.0)
  return t.1
}
";
    assert_eq!(
        check(text),
        Err("`s.0` is used after it was moved in `twice`".to_string())
    );
}

#[test]
fn a_missing_decref_is_rejected() {
    let text = "fn ignore(s.0: obj) -> int {\n  return 1\n}\n";
    assert_eq!(
        check(text),
        Err("`s.0` is still owned at the end of the function in `ignore`".to_string())
    );
}

#[test]
fn a_double_decref_is_rejected() {
    let text = "fn ignore(s.0: obj) -> int {\n  decref s.0\n  decref s.0\n  return 1\n}\n";
    assert_eq!(
        check(text),
        Err("`s.0` is released after it was moved in `ignore`".to_string())
    );
}

#[test]
fn a_dup_of_a_variable_that_is_not_rc_is_rejected() {
    let text = "fn f(n.0: int) -> int {\n  dup n.0\n  return n.0\n}\n";
    assert_eq!(
        check(text),
        Err("`n.0` is duplicated but is not reference counted in `f`".to_string())
    );
}

#[test]
fn drop_takes_the_ownership_of_its_value() {
    let once = "fn f(s.0: obj) -> unit {\n  let t.1: unit = drop s.0\n  return t.1\n}\n";
    assert_eq!(check(once), Ok(()));
    let twice = "\
fn g(s.0: obj) -> unit {
  let t.1: unit = drop s.0
  let u.2: unit = drop s.0
  return u.2
}
";
    assert_eq!(
        check(twice),
        Err("`s.0` is used after it was moved in `g`".to_string())
    );
}

#[test]
fn scopes_accept_a_value_used_twice_without_dup() {
    // Perceus より前の IR には `dup` がないので、所有は数えない
    let text = "\
fn twice(s.0: obj) -> obj {
  let t.1: obj = extern Prelude.++(s.0, s.0)
  return t.1
}
";
    assert_eq!(check_scopes(text), Ok(()));
}

#[test]
fn scopes_reject_a_dup() {
    let text = "\
fn twice(s.0: obj) -> obj {
  dup s.0
  let t.1: obj = extern Prelude.++(s.0, s.0)
  return t.1
}
";
    assert_eq!(
        check_scopes(text),
        Err("`s.0` is duplicated before Perceus in `twice`".to_string())
    );
}

#[test]
fn scopes_reject_a_decref() {
    let text = "fn ignore(s.0: obj) -> int {\n  decref s.0\n  return 1\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("`s.0` is released before Perceus in `ignore`".to_string())
    );
}

/// 組を分解し、`release` で先頭のフィールドだけを残す。
const RELEASE: &str = "\
layout (,) { (,)(tobj, tobj) }
fn f(p.0: obj) -> obj {
  unpack p.0 (,) #0(a.1: obj, b.2: obj)
  release p.0 (,) #0(a.1, _)
  return a.1
}
";

#[test]
fn scopes_reject_a_release() {
    assert_eq!(
        check_scopes(RELEASE),
        Err("`p.0` is released with its fields before Perceus in `f`".to_string())
    );
}

#[test]
fn a_release_that_keeps_no_field_is_rejected() {
    // テキストの形では書けないので、読んだ IR の項目を消す
    let mut program = read(RELEASE);
    let Stmt::Release { fields, .. } = &mut program.functions[0].blocks[0].stmts[1] else {
        panic!("the second statement is the release");
    };
    fields[0] = None;
    assert_eq!(
        verify(&program).map_err(|error| error.to_string()),
        Err("a release of `p.0` keeps no field in `f`".to_string())
    );
}

#[test]
fn a_release_that_keeps_a_field_that_is_not_rc_is_rejected() {
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn f(p.0: obj) -> int {
  unpack p.0 (,) #0(n.1: int, s.2: obj)
  release p.0 (,) #0(n.1, _)
  return n.1
}
";
    assert_eq!(
        check(text),
        Err("`n.1` is kept but is not reference counted in `f`".to_string())
    );
}

#[test]
fn a_constructor_value_without_fields_is_rejected() {
    let text = "layout Option { None, Some(tobj) }\nfn f() -> tobj {\n  let d.0: tobj = con Option #1()\n  return d.0\n}\n";
    let message =
        "a constructor value without fields is written as a tag `#N`, not `con` in `f`".to_string();
    assert_eq!(check_scopes(text), Err(message.clone()));
    assert_eq!(check(text), Err(message));
}

#[test]
fn scopes_reject_a_variable_used_outside_its_scope() {
    let text = "fn f() -> obj {\n  return s.0\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("`s.0` is used outside its scope in `f`".to_string())
    );
}

// switch

#[test]
fn a_switch_that_binds_fields_is_accepted() {
    let text = "\
layout Option { None, Some(tobj) }
fn f(d.0: tobj) -> obj {
  switch d.0 Option { #0 -> b1, #1(x.1: obj) -> b2 }
b1:
  decref d.0
  let e.2: obj = const \"e\"
  return e.2
b2:
  release d.0 Option #1(x.1)
  return x.1
}
";
    assert_eq!(check(text), Ok(()));
}

#[test]
fn a_switch_on_a_variable_that_is_not_rc_cannot_bind_fields() {
    let text = "\
layout Option { None, Some(tobj) }
fn f(d.0: enum) -> obj {
  switch d.0 Option { #0 -> b1, #1(x.1: obj) -> b2 }
b1:
  let e.2: obj = const \"e\"
  return e.2
b2:
  return x.1
}
";
    assert_eq!(
        check_scopes(text),
        Err(
            "`d.0` (enum) is not reference counted, but a switch binds its fields in `f`"
                .to_string()
        )
    );
}

#[test]
fn a_switch_on_a_constant_cannot_bind_fields() {
    // 定数の値はフィールドを持たないので、実行するとフィールドの数が合わずに止まる
    let text = "\
layout Option { None, Some(tobj) }
fn f() -> int {
  switch #1 Option { #1(x.0: obj) -> b1 }
b1:
  decref x.0
  return 1
}
";
    assert_eq!(
        check(text),
        Err("a switch on a constant binds fields in `f`".to_string())
    );
}

#[test]
fn a_switch_without_targets_is_rejected() {
    // 行き先のない `switch` は、所有している値を手放さないまま関数を終える
    let text = "fn f(s.0: obj, b.1: enum) -> unit {\n  switch b.1 {}\n}\n";
    assert_eq!(
        check(text),
        Err("a switch has no targets in `f`".to_string())
    );
    assert_eq!(
        check_scopes(text),
        Err("a switch has no targets in `f`".to_string())
    );
}

#[test]
fn a_literal_switch_with_a_default_is_accepted() {
    let text = "\
fn main(n.0: int) -> int {
  switch n.0 { 1 -> b1, 2 -> b2, _ -> b3 }
b1:
  return 10
b2:
  return 20
b3:
  return 0
}
";
    assert_eq!(check(text), Ok(()));
}

#[test]
fn a_literal_switch_without_a_default_is_rejected() {
    let text = "fn main(n.0: int) -> int {\n  switch n.0 { 1 -> b1 }\nb1:\n  return 10\n}\n";
    assert_eq!(
        check(text),
        Err("a switch on literals has no default in `main`".to_string())
    );
}

#[test]
fn a_switch_that_mixes_kinds_of_cases_is_rejected() {
    let text = "\
fn main(n.0: int) -> int {
  switch n.0 { #0 -> b1, 1 -> b2, _ -> b3 }
b1:
  return 10
b2:
  return 20
b3:
  return 0
}
";
    assert_eq!(
        check(text),
        Err("a switch mixes kinds of cases in `main`".to_string())
    );
}

#[test]
fn a_switch_with_two_cases_for_one_literal_is_rejected() {
    let text = "\
fn main(n.0: int) -> int {
  switch n.0 { 1 -> b1, 1 -> b2, _ -> b3 }
b1:
  return 10
b2:
  return 20
b3:
  return 0
}
";
    assert_eq!(
        check(text),
        Err("a switch has two cases for 1 in `main`".to_string())
    );
}

#[test]
fn a_literal_case_that_binds_fields_is_rejected() {
    let text = "\
fn main(n.0: int) -> int {
  switch n.0 { 1(x.1: int) -> b1, _ -> b2 }
b1:
  return x.1
b2:
  return 0
}
";
    assert_eq!(
        check(text),
        Err("a literal case binds fields in `main`".to_string())
    );
}

#[test]
fn a_string_case_without_its_constant_is_rejected() {
    let text = "\
fn main(s.0: obj) -> int {
  switch s.0 { \"a\" -> b1, _ -> b2 }
b1:
  return 1
b2:
  return 0
}
";
    let mut program = read(text);
    program.strings.clear();
    assert_eq!(
        verify(&program).map_err(|error| error.to_string()),
        Err("a case refers to string constant 0, which does not exist in `main`".to_string())
    );
}

#[test]
fn a_constant_without_its_string_is_rejected() {
    let mut program = read("fn f() -> obj {\n  let s.0: obj = const \"a\"\n  return s.0\n}\n");
    program.strings.clear();
    assert_eq!(
        verify_scopes(&program).map_err(|error| error.to_string()),
        Err("a constant refers to string 0, which does not exist in `f`".to_string())
    );
}

#[test]
fn a_switch_with_two_defaults_does_not_parse() {
    let text = "\
fn main(n.0: int) -> int {
  switch n.0 { 1 -> b1, _ -> b2, _ -> b3 }
b1:
  return 10
b2:
  return 0
b3:
  return 1
}
";
    let error = parse(text).unwrap_err().to_string();
    assert!(error.contains("two defaults"), "{error}");
}

// 呼び出し、クロージャ、extern

#[test]
fn a_direct_call_with_the_wrong_number_of_arguments_is_rejected() {
    let text = format!("{K}fn f() -> int {{\n  let t.0: int = call k(1, 2)\n  return t.0\n}}\n");
    assert_eq!(
        check(&text),
        Err("a direct call to `k` passes 2 arguments, but it takes 1 in `f`".to_string())
    );
}

#[test]
fn a_closure_without_arguments_is_rejected() {
    let text = format!("fn f() -> tobj {{\n  let c.0: tobj = closure k()\n  return c.0\n}}\n{K}");
    assert_eq!(
        check(&text),
        Err("a closure of `k` has no arguments; use `&k` in `f`".to_string())
    );
}

#[test]
fn a_closure_with_every_argument_is_rejected() {
    let text = format!("fn f() -> tobj {{\n  let c.0: tobj = closure k(1)\n  return c.0\n}}\n{K}");
    assert_eq!(
        check(&text),
        Err("a closure of `k` has 1 arguments, but it must have fewer than 1 in `f`".to_string())
    );
}

#[test]
fn an_extern_with_the_wrong_number_of_arguments_is_rejected() {
    // テキストの形は引数をいくつでも読み、数は verifier が表の値と比べる
    let text = "fn f(a.0: int) -> int {\n  let t.1: int = extern Prelude.+(a.0)\n  return t.1\n}\n";
    assert_eq!(
        check(text),
        Err("`Prelude.+` takes 2 arguments but is given 1 in `f`".to_string())
    );
}

#[test]
fn an_extern_chosen_by_type_is_rejected() {
    // `==` と `!=` は translate が比べ方ごとの行に置き換えるので、Core IR に届かない
    let text = "\
fn f(a.0: int, b.1: int) -> enum {
  let t.2: enum = extern Prelude.==(a.0, b.1)
  return t.2
}
";
    assert_eq!(
        check(text),
        Err("`Prelude.==` is chosen by type and must not reach Core IR in `f`".to_string())
    );
}

// エフェクト

#[test]
fn handlers_operations_and_resume_are_calls() {
    assert_eq!(check(&handler_program(ASK)), Ok(()));
}

#[test]
fn a_handler_has_a_clause_for_each_operation() {
    assert_eq!(
        check(&handler_program("effect Ask { ask/1, tell/1 }\n")),
        Err(
            "a handler of `Ask` has clauses for 1 operations, but the effect has 2 in `main`"
                .to_string()
        )
    );
}

#[test]
fn a_clause_receives_the_arguments_k_and_the_state_after_its_captures() {
    let text = "\
effect Ask { ask/1 }
fn f() -> int {
  let t.0: int = handle Ask((), &body) { ask: &clause } return &ret
  return t.0
}
fn body(u.0: unit) -> int {
  return 1
}
fn clause(x.0: int, k.1: tobj) -> int {
  tail resume k.1(x.0, ())
}
fn ret(x.0: int, s.1: unit) -> int {
  return x.0
}
";
    let expected = "the clause for `ask` needs 3 parameters after its captures (1 for the arguments, `k`, and the state), but it has 2 in `f`";
    assert_eq!(check(text), Err(expected.to_string()));
    // 節の引数の数は所有に関わらないので、Perceus より前の IR でも確かめる
    assert_eq!(check_scopes(text), Err(expected.to_string()));
}

#[test]
fn a_clause_of_a_never_operation_receives_the_arguments_and_the_state() {
    let text = "\
effect Fail { never fail/1 }
fn f(n.0: int) -> int {
  let c.1: tobj = closure clause(n.0)
  let t.2: int = handle Fail((), &body) { fail: c.1 } return &ret
  return t.2
}
fn body(u.0: unit) -> int {
  return 1
}
fn clause(n.0: int, x.1: int, k.2: tobj, s.3: unit) -> int {
  return n.0
}
fn ret(x.0: int, s.1: unit) -> int {
  return x.0
}
";
    assert_eq!(
        check(text),
        Err("the clause for `fail` needs 2 parameters after its captures (1 for the arguments and the state), but it has 3 in `f`".to_string())
    );
}

#[test]
fn a_return_clause_receives_the_value_and_the_state_after_its_captures() {
    let text = "\
effect Ask { ask/1 }
fn f(n.0: int) -> int {
  let c.1: tobj = closure ret(n.0)
  let t.2: int = handle Ask((), &body) { ask: &clause } return c.1
  return t.2
}
fn body(u.0: unit) -> int {
  return 1
}
fn clause(x.0: int, k.1: tobj, s.2: unit) -> int {
  tail resume k.1(x.0, s.2)
}
fn ret(n.0: int, x.1: int) -> int {
  return x.1
}
";
    assert_eq!(
        check(text),
        Err("the `return` clause needs 2 parameters after its captures (the value and the state), but it has 1 in `f`".to_string())
    );
}

#[test]
fn a_clause_outside_its_scope_is_rejected_before_its_parameters_are_counted() {
    // `c.1` は b1 だけで定義され、`handle` のある b3 を支配しない。引数の数も合わないが、先に範囲の誤りを報告する
    let text = "\
layout Prelude.Bool { False, True }
effect Ask { ask/1 }
fn f(k.0: enum) -> int {
  switch k.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let c.1: tobj = closure g(5)
  jump b3()
b2:
  jump b3()
b3:
  let t.2: int = handle Ask((), &body) { ask: c.1 } return &ret
  return t.2
}
fn g(y.0: int, x.1: int) -> int {
  return x.1
}
fn body(u.0: unit) -> int {
  return 1
}
fn ret(x.0: int, s.1: unit) -> int {
  return x.0
}
";
    assert_eq!(
        check_scopes(text),
        Err("`c.1` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn perform_names_an_operation_of_its_effect() {
    let text = format!("{ASK}fn f() -> int {{\n  tail perform Ask.#1()\n}}\n");
    assert_eq!(
        check(&text),
        Err("`perform` names operation 1 of `Ask`, which has 1 operations in `f`".to_string())
    );
}

#[test]
fn a_perform_must_agree_with_the_effect_on_resuming() {
    let resumes =
        "effect Fail { never fail/1 }\nfn f() -> int {\n  tail perform Fail.fail(())\n}\n";
    assert_eq!(
        check(resumes),
        Err("`Fail.fail` never resumes, but this perform resumes in `f`".to_string())
    );
    let never = "effect Ask { ask/1 }\nfn f() -> int {\n  tail perform never Ask.ask(1)\n}\n";
    assert_eq!(
        check(never),
        Err("`Ask.ask` resumes, but this perform never resumes in `f`".to_string())
    );
}

/// テキストは `handle` と `perform` の前の `mask` を読まないので、読んだ後で `mask` を付ける。
#[test]
fn a_mask_on_handle_or_perform_is_rejected() {
    let texts = [
        "effect Ask { ask/1 }\nfn f(b.0: tobj, c.1: tobj, r.2: tobj) -> int {\n  tail handle Ask((), b.0) { ask: c.1 } return r.2\n}\n",
        "effect Ask { ask/1 }\nfn f() -> int {\n  tail perform Ask.ask(1)\n}\n",
    ];
    let errors: Vec<String> = texts
        .into_iter()
        .map(|text| {
            let mut program = read(text);
            let Term::TailCall { call: _, mask } = &mut program.functions[0].blocks[0].term else {
                panic!("not a tail call");
            };
            mask.push(0);
            verify_scopes(&program).unwrap_err().message
        })
        .collect();
    assert_eq!(errors, ["a mask on handle", "a mask on perform"]);
}

#[test]
fn a_mask_must_name_known_effects_in_order() {
    let unknown = "\
effect Main.State { get/1, put/1 }
fn entry$main(c.0: tobj) -> unit {
  tail mask [#5] apply c.0(())
}
";
    let unordered = "\
effect Main.A { a/1 }
effect Main.B { b/1 }
fn entry$main(c.0: tobj) -> unit {
  tail mask [Main.B, Main.A] apply c.0(())
}
";
    assert_eq!(
        check(unknown),
        Err("a mask names an unknown effect #5 in `entry$main`".to_string())
    );
    assert_eq!(
        check(unordered),
        Err("a mask is not in ascending order in `entry$main`".to_string())
    );
}

#[test]
fn a_mask_may_name_one_effect_twice() {
    let text = "\
effect Main.A { a/1 }
effect Main.B { b/1 }
fn entry$main(c.0: tobj) -> unit {
  let t.1: tobj = mask [Main.A, Main.A, Main.B] apply c.0(())
  tail mask [Main.B] apply t.1(())
}
";
    assert_eq!(check(text), Ok(()));
}

// 大きさ

/// `n` 個の `switch` が続き、片方の枝が呼び出しをして合流する関数。所有の検査の段では、合流のブロックの数だけ、
/// 見える変数の集合を決め直す。`save` は呼び出しの `save` の部分で、Perceus より前の形では空にする。
fn chain_of_switches(n: u32, save: &str) -> String {
    let mut text = format!(
        "{BOOL}{K}fn f(s.0: obj, n.1: int, c.2: enum) -> obj {{\n  switch c.2 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}\n"
    );
    for i in 0..n {
        let (call, other, merge) = (3 * i + 1, 3 * i + 2, 3 * i + 3);
        text.push_str(&format!(
            "b{call}:\n  let t.{}: int = call k(n.1){save}\n  jump b{merge}()\nb{other}:\n  jump b{merge}()\nb{merge}:\n",
            i + 3
        ));
        if i + 1 < n {
            text.push_str(&format!(
                "  switch c.2 Prelude.Bool {{ #0 -> b{}, #1 -> b{} }}\n",
                merge + 1,
                merge + 2
            ));
        } else {
            text.push_str("  return s.0\n");
        }
    }
    text.push_str("}\n");
    text
}

#[test]
fn a_long_chain_of_switches_is_verified_in_linear_time() {
    // `lower` は debug ビルドで毎回 verify するので、ブロックの数に比例する時間で終わらなければならない。合流の
    // ブロックごとに所有や見える変数を関数の先頭から数え直すと、2乗の時間がかかる
    let before = read(&chain_of_switches(20000, ""));
    let after = read(&chain_of_switches(20000, " save [s.0, n.1, c.2]"));
    let start = std::time::Instant::now();
    assert_eq!(verify_scopes(&before), Ok(()));
    assert_eq!(verify(&after), Ok(()));
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "took {elapsed:?}"
    );
}

/// 借りたフィールドへの `switch` が `n` 段続く関数。どの段のフィールドも、引数の `d` を持ち主にする。
fn borrowed_chain(n: u32) -> String {
    let mut text =
        format!("{BOX}fn f(d.0: obj) -> int {{\n  switch d.0 Box {{ #0(y.1: obj) -> b1 }}\n");
    for i in 1..n {
        text.push_str(&format!(
            "b{i}:\n  switch y.{i} Box {{ #0(y.{}: obj) -> b{} }}\n",
            i + 1,
            i + 1
        ));
    }
    text.push_str(&format!(
        "b{n}:\n  dup y.{n}\n  decref d.0\n  decref y.{n}\n  return 1\n}}\n"
    ));
    text
}

#[test]
fn a_long_chain_of_switches_on_borrowed_fields_is_verified_in_linear_time() {
    // 借りた変数が有効かどうかは、変数と持ち主の所有を1回ずつ見れば決まる。親を1段ずつたどると2乗の時間がかかる
    let program = read(&borrowed_chain(100_000));
    let start = std::time::Instant::now();
    assert_eq!(verify(&program), Ok(()));
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "took {elapsed:?}"
    );
}

/// `c || c || ... || c` の形。`n` 個の `switch` が並び、真の辺はそれぞれ辺のブロックを通って1つの合流のブロックへ
/// 入る。i 番目の辺のブロックは支配木の深さ i + 1 にあるので、合流のブロックには深さの違う `n` 本の辺が入る。
fn or_chain(n: u32) -> String {
    let merge = 2 * n + 1;
    let mut text = format!("{BOOL}fn f(c.0: enum) -> int {{\n");
    for i in 0..n {
        if i > 0 {
            text.push_str(&format!("b{}:\n", 2 * i));
        }
        let edge = 2 * i + 1;
        let next = if i + 1 < n { 2 * i + 2 } else { 2 * n };
        text.push_str(&format!(
            "  switch c.0 Prelude.Bool {{ #1 -> b{edge}, #0 -> b{next} }}\nb{edge}:\n  jump b{merge}(1)\n"
        ));
    }
    text.push_str(&format!(
        "b{}:\n  jump b{merge}(0)\nb{merge}(r.1: int):\n  return r.1\n}}\n",
        2 * n
    ));
    text
}

#[test]
fn many_deep_jumps_into_one_block_are_verified_in_near_linear_time() {
    // 合流のブロックの支配者を、入る辺ごとに支配木を1段ずつ登って求めると、深さの和だけ、つまり2乗の時間がかかる
    let program = read(&or_chain(100_000));
    let start = std::time::Instant::now();
    assert_eq!(verify_scopes(&program), Ok(()));
    assert_eq!(verify(&program), Ok(()));
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "took {elapsed:?}"
    );
}

#[test]
fn a_long_run_of_if_statements_is_verified_in_linear_time() {
    // 文の `if` が続くと、`switch` と合流するブロックが文の数だけ連なる。`lower` はデバッグビルドで毎回 verify するので、
    // 文の数に比例する時間で終わらなければならない。条件は引数にする。`if True` は translate がその場で枝を選び、
    // `switch` を出さないためである
    const COUNT: usize = 20000;
    let mut text = String::from("step : Bool -> <IO> Unit\nstep b =\n  let s = \"keep\"\n");
    text.push_str(&"  if b then println \"x\"\n".repeat(COUNT));
    text.push_str("  println s\n\nmain : Unit -> <IO> Unit\nmain () = step True");
    let start = std::time::Instant::now();
    let program = eml_test_support::core(&text);
    assert_eq!(verify(&program), Ok(()));
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "took {elapsed:?}"
    );
    let step = program
        .functions
        .iter()
        .find(|function| function.name == "step")
        .expect("step");
    let switches = step
        .blocks
        .iter()
        .filter(|block| matches!(block.term, Term::Switch { .. }))
        .count();
    assert_eq!(switches, COUNT);
}
