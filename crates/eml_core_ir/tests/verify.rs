//! Core IR のテキストで書いた IR で、verifier が正しいものを受け入れ、壊れたものを拒むことを確かめる (docs/spec/core-ir.md)。
//! 構造の規則 R1 から R9 と、前の形の IR から引き継いだ検査を1つずつ確かめる。

use eml_core_ir::{Program, Stmt, Term, parse, verify, verify_scopes, verify_translated};

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

fn check_translated(text: &str) -> Result<(), String> {
    verify_translated(&read(text)).map_err(|error| error.to_string())
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

/// `handle ask 1 with | ask x k -> resume k x` を持ち上げた形。`effects` は先頭のエフェクトの行。節と本体は値として
/// 使うので一様で、捕まえる `Int` と handle の結果は `box` と `unbox` を通る。
fn handler_program(effects: &str) -> String {
    format!(
        "{effects}fn main() -> int {{
  let b.0: tobj = box 1
  let c.1: tobj = closure main$handle0(b.0)
  let b.2: tobj = box 2
  let c.3: tobj = closure main$handle0$ask(b.2)
  let r.4: tobj = handle Ask((), c.1) {{ ask: c.3 }} return &main$handle0$return
  let t.5: int = unbox r.4
  decref r.4
  return t.5
}}
fn main$handle0(n.0: tobj, p.1: unit) -> tobj {{
  tail perform Ask.ask(n.0)
}}
fn main$handle0$ask(m.0: tobj, x.1: tobj, k.2: tobj, s.3: unit) -> tobj {{
  decref m.0
  tail resume k.2(x.1, s.3)
}}
fn main$handle0$return(x.0: tobj, s.1: unit) -> tobj {{
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
        Err(
            "an unpack of `p.0` as `Box` #0 has 0 fields, but the constructor has 1 in `f`"
                .to_string()
        )
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

/// `obj` の値と `unit` の値と `()` を、`tobj` の引数に渡す。`decrefs` は、所有の段で受けた値を手放す文である。
fn compatible_jump(decrefs: &str) -> String {
    format!(
        "fn f(s.0: obj, u.1: unit) -> tobj {{
  jump b1(s.0, u.1, ())
b1(a.2: tobj, b.3: tobj, c.4: tobj):
{decrefs}  return a.2
}}
"
    )
}

#[test]
fn a_jump_passes_values_to_compatible_parameters() {
    // `obj` と `tobj`、`unit` と `tobj` は互換で、命令なしで行き来する (docs/spec/core-ir.md の「値の表現」)
    assert_eq!(check_translated(&compatible_jump("")), Ok(()));
    assert_eq!(check_scopes(&compatible_jump("")), Ok(()));
    assert_eq!(
        check(&compatible_jump("  decref b.3\n  decref c.4\n")),
        Ok(())
    );
}

#[test]
fn a_returned_value_is_compatible_with_the_function() {
    let text = "\
fn f() -> tobj {
  return ()
}
fn g(s.0: obj) -> tobj {
  return s.0
}
fn h(t.0: tobj) -> obj {
  return t.0
}
fn i(u.0: unit) -> tobj {
  return u.0
}
";
    assert_eq!(check_translated(text), Ok(()));
    assert_eq!(check_scopes(text), Ok(()));
    assert_eq!(check(text), Ok(()));
}

#[test]
fn a_returned_value_that_is_not_compatible_is_rejected() {
    // `Int` の定数は `tobj` に収まらない。`unit` は `tobj` と互換でも、`obj` とは互換でない
    for (text, message) in [
        (
            "fn f() -> tobj {\n  return 5\n}\n",
            "5 is returned from a function that returns tobj in `f`",
        ),
        (
            "fn f() -> obj {\n  return ()\n}\n",
            "() is returned from a function that returns obj in `f`",
        ),
        (
            "fn f(u.0: unit) -> obj {\n  return u.0\n}\n",
            "`u.0` (unit) is returned from a function that returns obj in `f`",
        ),
    ] {
        assert_eq!(check_translated(text), Err(message.to_string()));
        assert_eq!(rejected_at_both_levels(text), message);
    }
}

#[test]
fn the_uses_of_a_never_perform_binder_are_not_compared() {
    // `never` の操作の `perform` の束縛の使いには制御が届かないので、互換の位置 (`return` の値、`jump` の実引数、
    // 呼び出しの引数) で位置と比べない。縮約は、`f` の形をそのまま `tail perform never` にする
    let text = "\
effect Fail { never fail/1 }
fn f(s.0: obj) -> tobj {
  let t.1: int = perform never Fail.fail(s.0)
  return t.1
}
fn g(s.0: obj) -> tobj {
  let t.1: int = perform never Fail.fail(s.0)
  jump b1(t.1)
b1(a.2: tobj):
  return a.2
}
fn h(s.0: obj) -> tobj {
  let t.1: int = perform never Fail.fail(s.0)
  let r.2: tobj = call k(t.1)
  return r.2
}
fn k(x.0: tobj) -> tobj {
  return x.0
}
";
    assert_eq!(check_translated(text), Ok(()));
    assert_eq!(check_scopes(text), Ok(()));
    assert_eq!(check(text), Ok(()));
}

#[test]
fn the_result_of_a_call_is_compared_with_the_callee() {
    let text = format!("{K}fn f() -> obj {{\n  let t.0: obj = call k(1)\n  return t.0\n}}\n");
    assert_eq!(
        rejected_at_both_levels(&text),
        "`t.0` (obj) is bound to `k`, which returns int in `f`"
    );
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

// 境界の検査

/// 値として使う一様な関数。`i` は本体に、`two` は `return` の節と `get/0` の節に、`three` は `ask/1` の節に使える。
const UNIFORM: &str = "\
fn i(u.0: unit) -> unit {
  return u.0
}
fn two(a.0: unit, b.1: unit) -> unit {
  return a.0
}
fn three(a.0: unit, b.1: unit, c.2: unit) -> unit {
  return a.0
}
";

/// 引数が `params` で本体が `body` の `f` に、エフェクト `Ask`、`State`、`Fail` と一様な関数を添える。
fn calling(params: &str, body: &str) -> String {
    format!(
        "{ASK}effect State {{ get/0 }}\neffect Fail {{ never fail/1 }}\nfn f({params}) -> tobj {{\n{body}}}\n{UNIFORM}"
    )
}

/// `tobj` を2つ受けて手放す関数。
const TAKES_TOBJ: &str =
    "fn g(a.0: tobj, b.1: tobj) -> unit {\n  decref a.0\n  decref b.1\n  return ()\n}\n";

#[test]
fn the_arguments_of_a_direct_call_are_compatible_with_the_parameters() {
    let call = |args: &str| {
        format!(
            "\
fn f(s.0: obj, x.1: int, u.2: unit) -> unit {{
  let t.3: unit = call g({args})
  return t.3
}}
{TAKES_TOBJ}"
        )
    };
    // `obj` の変数、`unit` の変数、`()` は、命令なしで `tobj` の引数に渡せる
    assert_eq!(check(&call("s.0, u.2")), Ok(()));
    assert_eq!(check(&call("s.0, ()")), Ok(()));
    assert_eq!(
        rejected_at_both_levels(&call("x.1, ()")),
        "argument 0 of `g` is `x.1` (int), but the function takes tobj in `f`"
    );
    assert_eq!(
        rejected_at_both_levels(&call("s.0, 5")),
        "argument 1 of `g` is 5, but the function takes tobj in `f`"
    );
}

#[test]
fn the_result_of_a_call_is_bound_to_a_compatible_variable() {
    // `tobj` を返す関数の結果は、`obj` の変数でも `unit` の変数でも受けられる
    let text = "\
fn f() -> unit {
  let s.0: obj = call h()
  let u.1: unit = call h() save [s.0]
  decref s.0
  return u.1
}
fn h() -> tobj {
  return ()
}
";
    assert_eq!(check(text), Ok(()));
}

#[test]
fn the_arguments_of_a_closure_are_compatible_with_the_parameters() {
    let closure = |arg: &str| {
        format!(
            "\
fn f(s.0: obj, n.1: int) -> tobj {{
  let c.2: tobj = closure g({arg})
  return c.2
}}
fn g(a.0: tobj, b.1: tobj) -> tobj {{
  decref b.1
  return a.0
}}
"
        )
    };
    assert_eq!(check(&closure("s.0")), Ok(()));
    assert_eq!(
        rejected_at_both_levels(&closure("n.1")),
        "argument 0 of a closure of `g` is `n.1` (int), but the function takes tobj in `f`"
    );
}

#[test]
fn a_function_used_as_a_value_is_uniform() {
    let value = |atom: &str| {
        format!(
            "\
fn f() -> tobj {{
  return {atom}
}}
{K}fn h(a.0: tobj) -> int {{
  let n.1: int = unbox a.0
  decref a.0
  return n.1
}}
{UNIFORM}"
        )
    };
    assert_eq!(check(&value("&i")), Ok(()));
    assert_eq!(
        rejected_at_both_levels(&value("&k")),
        "`k` is used as a function value, but its parameter 0 is int in `f`"
    );
    assert_eq!(
        rejected_at_both_levels(&value("&h")),
        "`h` is used as a function value, but it returns int in `f`"
    );
}

#[test]
fn the_translated_level_allows_a_function_value_that_is_not_uniform() {
    // translate は `if c then double else inc` を `jump b3(&double)` にする。一様にするのは box の挿入である
    let text = format!("fn f() -> tobj {{\n  jump b1(&k)\nb1(h.0: tobj):\n  return h.0\n}}\n{K}");
    assert_eq!(check_translated(&text), Ok(()));
    assert_eq!(
        rejected_at_both_levels(&text),
        "`k` is used as a function value, but its parameter 0 is int in `f`"
    );
}

#[test]
fn a_function_value_that_is_dropped_or_switched_on_is_uniform() {
    // 値を呼び出しに渡さない `drop` と case のない `switch` でも、`&k` は関数の値である
    for body in [
        "  let u.0: unit = drop &k\n  return u.0\n",
        "  switch &k { _ -> b1 }\nb1:\n  return ()\n",
    ] {
        let text = format!("fn f() -> unit {{\n{body}}}\n{K}");
        assert_eq!(check_translated(&text), Ok(()));
        assert_eq!(
            rejected_at_both_levels(&text),
            "`k` is used as a function value, but its parameter 0 is int in `f`"
        );
    }
}

#[test]
fn a_closure_of_a_function_that_is_not_uniform_is_rejected_before_its_arguments() {
    // 引数の数の後で対象が一様かを確かめ、その後で引数の Repr と範囲を確かめる
    let closure = |g: &str| {
        format!(
            "\
fn f() -> tobj {{
  let c.0: tobj = closure g(c.0)
  return c.0
}}
fn g(a.0: {g}, b.1: {g}) -> {g} {{
  return a.0
}}
"
        )
    };
    assert_eq!(
        rejected_at_both_levels(&closure("int")),
        "`g` is used as a function value, but its parameter 0 is int in `f`"
    );
    assert_eq!(
        rejected_at_both_levels(&closure("tobj")),
        "`c.0` is used outside its scope in `f`"
    );
}

#[test]
fn the_operands_of_uniform_calls_are_compatible_with_tobj() {
    for (params, body, message) in [
        (
            "c.0: tobj, n.1: int",
            "  let t.2: tobj = apply n.1(())\n  return t.2\n",
            "the callee of an apply is `n.1` (int), but an apply takes tobj",
        ),
        (
            "c.0: tobj, n.1: int",
            "  let t.2: tobj = apply c.0(n.1)\n  return t.2\n",
            "argument 0 of an apply is `n.1` (int), but an apply takes tobj",
        ),
        (
            "",
            "  let t.0: tobj = perform Ask.ask(1)\n  return t.0\n",
            "argument 0 of a perform of `Ask.ask` is 1, but a perform takes tobj",
        ),
        (
            "k.0: tobj, n.1: int, s.2: int",
            "  let t.3: tobj = resume n.1((), ())\n  return t.3\n",
            "the continuation of a resume is `n.1` (int), but a resume takes tobj",
        ),
        (
            "k.0: tobj, n.1: int, s.2: int",
            "  let t.3: tobj = resume k.0(1, ())\n  return t.3\n",
            "the value of a resume is 1, but a resume takes tobj",
        ),
        (
            "k.0: tobj, n.1: int, s.2: int",
            "  let t.3: tobj = resume k.0((), s.2)\n  return t.3\n",
            "the state of a resume is `s.2` (int), but a resume takes tobj",
        ),
        (
            "",
            "  let t.0: tobj = handle State(0, &i) { get: &two } return &two\n  return t.0\n",
            "the initial state of a handler of `State` is 0, but a handler takes tobj",
        ),
        (
            "u.0: unit, b.1: int",
            "  let t.2: tobj = handle Ask((), b.1) { ask: &three } return &two\n  return t.2\n",
            "the body of a handler of `Ask` is `b.1` (int), but a handler takes tobj",
        ),
        (
            "u.0: unit, b.1: tobj, c.2: int",
            "  let t.3: tobj = handle Ask((), b.1) { ask: c.2 } return &two\n  return t.3\n",
            "the clause for `ask` of a handler of `Ask` is `c.2` (int), but a handler takes tobj",
        ),
        (
            "u.0: unit, b.1: tobj, c.2: tobj, r.3: int",
            "  let t.4: tobj = handle Ask((), b.1) { ask: c.2 } return r.3\n  return t.4\n",
            "the `return` clause of a handler of `Ask` is `r.3` (int), but a handler takes tobj",
        ),
        (
            "",
            "  let t.0: int = perform never Fail.fail(1)\n  return ()\n",
            "argument 0 of a perform of `Fail.fail` is 1, but a perform takes tobj",
        ),
    ] {
        assert_eq!(
            rejected_at_both_levels(&calling(params, body)),
            format!("{message} in `f`")
        );
    }
}

#[test]
fn the_results_of_uniform_calls_are_bound_to_variables_compatible_with_tobj() {
    for (params, body, message) in [
        (
            "",
            "  let t.0: int = apply &i(())\n  return ()\n",
            "`t.0` (int) is bound to an apply, which returns tobj",
        ),
        (
            "",
            "  let t.0: int = perform Ask.ask(())\n  return ()\n",
            "`t.0` (int) is bound to a perform of `Ask.ask`, which returns tobj",
        ),
        (
            "k.0: tobj",
            "  let t.1: int = resume k.0((), ())\n  return ()\n",
            "`t.1` (int) is bound to a resume, which returns tobj",
        ),
        (
            "",
            "  let t.0: int = handle Ask((), &i) { ask: &three } return &two\n  return ()\n",
            "`t.0` (int) is bound to a handler of `Ask`, which returns tobj",
        ),
    ] {
        assert_eq!(
            rejected_at_both_levels(&calling(params, body)),
            format!("{message} in `f`")
        );
    }
}

#[test]
fn a_never_perform_has_no_result_to_compare() {
    // `never` の操作の `perform` は値を返さないので、束縛はどの Repr でもよく、`tail` はどの `ret` の関数にも置ける
    let text = "\
effect Fail { never fail/1 }
fn f() -> int {
  let t.0: int = perform never Fail.fail(())
  return t.0
}
fn g() -> int {
  tail perform never Fail.fail(())
}
";
    assert_eq!(check_scopes(text), Ok(()));
    assert_eq!(check(text), Ok(()));
}

#[test]
fn the_result_of_a_tail_call_is_compatible_with_the_function() {
    let tail = |ret: &str, params: &str, call: &str| {
        format!("{ASK}fn f({params}) -> {ret} {{\n  tail {call}\n}}\n{K}{UNIFORM}")
    };
    // `unit` を返す関数は、`tobj` を返す呼び出しで終われる
    assert_eq!(check(&tail("unit", "c.0: tobj", "apply c.0(())")), Ok(()));
    for (ret, params, call, message) in [
        (
            "tobj",
            "",
            "call k(1)",
            "a tail call to `k` returns int, but this function returns tobj",
        ),
        (
            "int",
            "c.0: tobj",
            "apply c.0(())",
            "a tail apply returns tobj, but this function returns int",
        ),
        (
            "int",
            "",
            "perform Ask.ask(())",
            "a tail perform of `Ask.ask` returns tobj, but this function returns int",
        ),
        (
            "int",
            "k.0: tobj",
            "resume k.0((), ())",
            "a tail resume returns tobj, but this function returns int",
        ),
        (
            "int",
            "",
            "handle Ask((), &i) { ask: &three } return &two",
            "a tail handler of `Ask` returns tobj, but this function returns int",
        ),
    ] {
        assert_eq!(
            rejected_at_both_levels(&tail(ret, params, call)),
            format!("{message} in `f`")
        );
    }
}

// R9: データの配置

/// どちらの段でも同じ誤りになることを確かめ、文言から末尾の `` in `f` `` を除いて返す。
fn layout_error(text: &str) -> String {
    let error = rejected_at_both_levels(text);
    error
        .strip_suffix(" in `f`")
        .unwrap_or_else(|| panic!("{error}"))
        .to_string()
}

/// 終端が `switch` だけの関数 `f`。行き先 b1, b2, .. は、それぞれの番号を返す。`head` は先頭の配置の行である。
fn switching(head: &str, params: &str, switch: &str) -> String {
    let targets = switch.matches("-> b").count();
    let blocks: String = (1..=targets)
        .map(|block| format!("b{block}:\n  return {block}\n"))
        .collect();
    format!("{head}fn f({params}) -> int {{\n  {switch}\n{blocks}}}\n")
}

#[test]
fn every_layout_reference_is_in_the_table() {
    let con = "fn f(x.0: tobj) -> obj {\n  let d.1: obj = con #3 #0(x.0)\n  return d.1\n}\n";
    assert_eq!(layout_error(con), "a con refers to the unknown layout #3");
    let switch = switching("", "d.0: tobj", "switch d.0 #3 { #0 -> b1, _ -> b2 }");
    assert_eq!(
        layout_error(&switch),
        "a switch refers to the unknown layout #3"
    );
    let literals = switching("", "n.0: int", "switch n.0 #3 { 1 -> b1, _ -> b2 }");
    assert_eq!(
        layout_error(&literals),
        "a switch refers to the unknown layout #3"
    );
    let unpack = "fn f(p.0: obj) -> int {\n  unpack p.0 #3 #0(x.1: tobj)\n  return 0\n}\n";
    assert_eq!(
        layout_error(unpack),
        "an unpack refers to the unknown layout #3"
    );
    let release = "\
layout Box { Box(tobj) }
fn f(p.0: obj) -> obj {
  unpack p.0 Box #0(x.1: obj)
  release p.0 #3 #0(x.1)
  return x.1
}
";
    assert_eq!(
        check(release),
        Err("a release refers to the unknown layout #3 in `f`".to_string())
    );
}

#[test]
fn only_a_switch_with_tag_cases_has_a_layout() {
    let tags = switching("", "c.0: enum", "switch c.0 { #0 -> b1, #1 -> b2 }");
    assert_eq!(layout_error(&tags), "a switch with tag cases has no layout");
    let message = "a switch without tag cases has the layout `Prelude.Bool`";
    let literals = switching(
        BOOL,
        "n.0: int",
        "switch n.0 Prelude.Bool { 1 -> b1, _ -> b2 }",
    );
    assert_eq!(layout_error(&literals), message);
    // `default` だけの `switch` は値を比べないので、配置を持たない
    let default = switching(BOOL, "c.0: enum", "switch c.0 Prelude.Bool { _ -> b1 }");
    assert_eq!(layout_error(&default), message);
    let default = switching("", "c.0: enum", "switch c.0 { _ -> b1 }");
    assert_eq!(check_scopes(&default), Ok(()));
    assert_eq!(check(&default), Ok(()));
}

#[test]
fn tags_are_in_the_range_of_their_layout() {
    let con = format!(
        "{BOOL}fn f(x.0: tobj) -> obj {{\n  let d.1: obj = con Prelude.Bool #2(x.0)\n  return d.1\n}}\n"
    );
    assert_eq!(
        layout_error(&con),
        "a con names #2, but `Prelude.Bool` has 2 constructors"
    );
    let case = switching(
        BOOL,
        "c.0: enum",
        "switch c.0 Prelude.Bool { #0 -> b1, #2 -> b2, _ -> b3 }",
    );
    assert_eq!(
        layout_error(&case),
        "a case names #2, but `Prelude.Bool` has 2 constructors"
    );
    let unpack =
        format!("{BOX}fn f(p.0: obj) -> int {{\n  unpack p.0 Box #1(x.1: tobj)\n  return 0\n}}\n");
    assert_eq!(
        layout_error(&unpack),
        "an unpack names #1, but `Box` has 1 constructors"
    );
}

/// フィールドの Repr が `tobj` と `int` の配置。
const PAIR_INT: &str = "layout Pair { Pair(tobj, int) }\n";

#[test]
fn every_instruction_has_the_fields_of_its_constructor() {
    let con = format!(
        "{OPTION}fn f(x.0: tobj) -> tobj {{\n  let d.1: tobj = con Option #1(x.0, x.0)\n  return d.1\n}}\n"
    );
    assert_eq!(
        layout_error(&con),
        "a con of `Option` #1 has 2 fields, but the constructor has 1"
    );
    let case = switching(
        OPTION,
        "d.0: tobj",
        "switch d.0 Option { #0 -> b1, #1 -> b2 }",
    );
    assert_eq!(
        layout_error(&case),
        "a case of `Option` #1 has 0 fields, but the constructor has 1"
    );
    let unpack = format!(
        "{PAIR_INT}fn f(p.0: obj) -> int {{\n  unpack p.0 Pair #0(a.1: tobj)\n  return 0\n}}\n"
    );
    assert_eq!(
        layout_error(&unpack),
        "an unpack of `p.0` as `Pair` #0 has 1 fields, but the constructor has 2"
    );
    let release = "\
layout Option { None, Some(tobj) }
fn f(d.0: tobj) -> obj {
  switch d.0 Option { #0 -> b1, #1(x.1: obj) -> b2 }
b1:
  decref d.0
  let e.2: obj = const \"e\"
  return e.2
b2:
  release d.0 Option #1(x.1, _)
  return x.1
}
";
    assert_eq!(
        check(release),
        Err(
            "a release of `d.0` as `Option` #1 has 2 fields, but the constructor has 1 in `f`"
                .to_string()
        )
    );
}

#[test]
fn a_tag_switch_is_on_a_variable_with_the_repr_of_its_layout() {
    let repr = switching(
        BOOL,
        "c.1: int",
        "switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }",
    );
    assert_eq!(
        layout_error(&repr),
        "`c.1` (int) is switched on as `Prelude.Bool`, which is enum"
    );
    let constant = switching(OPTION, "", "switch #1 Option { #0 -> b1, _ -> b2 }");
    assert_eq!(
        layout_error(&constant),
        "a switch on `Option` has the constant #1 as its scrutinee"
    );
}

#[test]
fn a_tag_switch_without_a_default_has_a_case_for_every_constructor() {
    let text = switching(
        OPTION,
        "d.0: tobj",
        "switch d.0 Option { #1(x.1: obj) -> b1 }",
    );
    assert_eq!(
        layout_error(&text),
        "a switch on `Option` has no default and no case for #0"
    );
}

#[test]
fn a_literal_switch_is_on_the_repr_of_its_literals() {
    let cases = [
        (
            "s.0: obj",
            "switch s.0 { 1 -> b1, _ -> b2 }",
            "`s.0` (obj) is switched on Int literals",
        ),
        (
            "n.0: int",
            "switch n.0 { \"a\" -> b1, _ -> b2 }",
            "`n.0` (int) is switched on String literals",
        ),
        (
            "",
            "switch () { 1 -> b1, _ -> b2 }",
            "() is switched on Int literals",
        ),
    ];
    for (params, switch, message) in cases {
        assert_eq!(layout_error(&switching("", params, switch)), message);
    }
}

#[test]
fn an_unpack_names_a_layout_with_one_constructor_with_fields() {
    let enumeration =
        "layout U { U }\nfn f(p.0: obj) -> int {\n  unpack p.0 U #0(x.1: tobj)\n  return 0\n}\n";
    assert_eq!(
        layout_error(enumeration),
        "`p.0` (obj) is unpacked as `U`, which is enum"
    );
    let shape = "\
layout Shape { Dot(int), Box(int, int) }
fn f(s.0: obj) -> int {
  unpack s.0 Shape #0(n.1: int)
  return n.1
}
";
    assert_eq!(
        layout_error(shape),
        "an unpack of `s.0` names `Shape`, which has 2 constructors"
    );
}

#[test]
fn a_con_binds_the_repr_of_its_layout() {
    let text = format!(
        "{OPTION}fn f(x.0: tobj) -> obj {{\n  let d.1: obj = con Option #1(x.0)\n  return d.1\n}}\n"
    );
    assert_eq!(
        layout_error(&text),
        "`d.1` (obj) is bound to a con of `Option`, which is tobj"
    );
}

#[test]
fn a_field_that_is_not_tobj_has_its_declared_repr() {
    let message = "field 1 of `Pair` #0 is `n.2` (obj), but the layout has int";
    let unpack = format!(
        "{PAIR_INT}fn f(p.0: obj) -> int {{\n  unpack p.0 Pair #0(a.1: tobj, n.2: obj)\n  return 0\n}}\n"
    );
    assert_eq!(layout_error(&unpack), message);
    let case = switching(
        PAIR_INT,
        "p.0: obj",
        "switch p.0 Pair { #0(a.1: tobj, n.2: obj) -> b1 }",
    );
    assert_eq!(layout_error(&case), message);
    let con = format!(
        "{PAIR_INT}fn f(x.0: tobj) -> obj {{\n  let p.1: obj = con Pair #0(x.0, ())\n  return p.1\n}}\n"
    );
    assert_eq!(
        layout_error(&con),
        "argument 1 of a con of `Pair` #0 is (), but the layout has int"
    );
}

#[test]
fn a_tobj_field_takes_an_obj_variable() {
    // `obj` の値は変換なしで `tobj` のフィールドに置け、`tobj` のフィールドは `obj` の変数に束縛できる
    let text = "\
layout Pair { Pair(tobj, int) }
fn f(s.0: obj) -> obj {
  let p.1: obj = con Pair #0(s.0, 1)
  unpack p.1 Pair #0(t.2: obj, n.3: int)
  return p.1
}
";
    assert_eq!(check_scopes(text), Ok(()));
    assert_eq!(check(text), Ok(()));
}

#[test]
fn a_tobj_field_takes_only_values_compatible_with_tobj() {
    // `tobj` のフィールドは互換の位置なので、`int` の変数と `Int` の定数は `box` してから置く
    let case = switching(
        OPTION,
        "d.0: tobj",
        "switch d.0 Option { #0 -> b1, #1(x.1: int) -> b2 }",
    );
    assert_eq!(
        layout_error(&case),
        "field 0 of `Option` #1 is `x.1` (int), but the layout has tobj"
    );
    let unpack =
        format!("{BOX}fn f(p.0: obj) -> int {{\n  unpack p.0 Box #0(n.1: int)\n  return n.1\n}}\n");
    assert_eq!(
        layout_error(&unpack),
        "field 0 of `Box` #0 is `n.1` (int), but the layout has tobj"
    );
    let con = |arg: &str| {
        format!(
            "{OPTION}fn f() -> tobj {{\n  let o.0: tobj = con Option #1({arg})\n  return o.0\n}}\n"
        )
    };
    assert_eq!(
        layout_error(&con("5")),
        "argument 0 of a con of `Option` #1 is 5, but the layout has tobj"
    );
    // `()` は互換の位置の `tobj` に収まる
    assert_eq!(check_scopes(&con("()")), Ok(()));
    assert_eq!(check(&con("()")), Ok(()));
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
        "  let t.4: obj = apply x.2(())\n  return t.4\n",
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
    let not_field = |field: &str, slot: usize, value: &str, layout: &str, tag: u32| {
        Err(format!(
            "`{field}` is not field {slot} of `{value}` as `{layout}` #{tag} in `f`"
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
    assert_eq!(check(grandchild), not_field("z.2", 0, "xs.0", "Box", 0));
    // 2回目の `unpack` の位置 0 のフィールドを、位置 1 に書く
    assert_eq!(
        check(&unpacking(
            "  unpack p.0 (,) #0(c.3: obj, d.4: tobj)\n  release p.0 (,) #0(a.1, c.3)\n  return a.1\n"
        )),
        not_field("c.3", 1, "p.0", "(,)", 0)
    );
    // 範囲の外のタグと違う数は、出どころを見る前に配置で拒む
    assert_eq!(
        check(&unpacking("  release p.0 (,) #1(a.1, _)\n  return a.1\n")),
        Err("a release names #1, but `(,)` has 1 constructors in `f`".to_string())
    );
    assert_eq!(
        check(&unpacking("  release p.0 (,) #0(a.1)\n  return a.1\n")),
        Err(
            "a release of `p.0` as `(,)` #0 has 1 fields, but the constructor has 2 in `f`"
                .to_string()
        )
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
    assert_eq!(check(other), not_field("b.3", 0, "p.0", "Box", 0));
    // 同じタグと位置でも、ほかの配置で分解したフィールドではない
    let layout = "\
layout Box { Box(tobj) }
layout Cell { Cell(tobj) }
fn f(p.0: obj) -> obj {
  unpack p.0 Box #0(a.1: obj)
  release p.0 Cell #0(a.1)
  return a.1
}
";
    assert_eq!(check(layout), not_field("a.1", 0, "p.0", "Cell", 0));
    // 同じ配置と位置でも、ほかのタグで分解したフィールドではない
    let tag = "\
layout Either { Left(tobj), Right(tobj) }
fn f(d.0: obj) -> obj {
  switch d.0 Either { #0(x.1: obj) -> b1, #1(y.2: obj) -> b2 }
b1:
  release d.0 Either #1(x.1)
  return x.1
b2:
  release d.0 Either #1(y.2)
  return y.2
}
";
    assert_eq!(check(tag), not_field("x.1", 0, "d.0", "Either", 1));
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
layout P { P(int, tobj) }
fn f(p.0: obj) -> int {
  unpack p.0 P #0(n.1: int, s.2: obj)
  release p.0 P #0(n.1, _)
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
fn f(n.0: tobj) -> int {
  let c.1: tobj = closure clause(n.0)
  let t.2: int = handle Fail((), &body) { fail: c.1 } return &ret
  return t.2
}
fn body(u.0: unit) -> int {
  return 1
}
fn clause(n.0: tobj, x.1: tobj, k.2: tobj, s.3: unit) -> tobj {
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
fn f(n.0: tobj) -> int {
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
fn ret(n.0: tobj, x.1: tobj) -> tobj {
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
  let c.1: tobj = closure g(())
  jump b3()
b2:
  jump b3()
b3:
  let t.2: int = handle Ask((), &body) { ask: c.1 } return &ret
  return t.2
}
fn g(y.0: unit, x.1: tobj) -> tobj {
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

// 変換の段

#[test]
fn the_translated_level_rejects_tail_calls() {
    // 末尾呼び出しは縮約だけが作る。box の挿入は、末尾呼び出しのない入力の `let` と `return` から末尾の位置を見る
    let handler = "\
fn body(u.0: unit) -> tobj {
  return ()
}
fn clause(s.0: obj, k.1: tobj, t.2: unit) -> tobj {
  return ()
}
fn ret(v.0: tobj, t.1: unit) -> tobj {
  return v.0
}
";
    for (text, message) in [
        (
            "fn f(x.0: int) -> int {\n  tail call f(x.0)\n}\n".to_string(),
            "a tail call is formed before contract in `f`",
        ),
        (
            "fn f(c.0: tobj) -> tobj {\n  tail apply c.0(())\n}\n".to_string(),
            "a tail apply is formed before contract in `f`",
        ),
        (
            "effect Ask { ask/1 }\nfn f(s.0: obj) -> tobj {\n  tail perform Ask.ask(s.0)\n}\n"
                .to_string(),
            "a tail perform is formed before contract in `f`",
        ),
        (
            "fn f(k.0: tobj) -> tobj {\n  tail resume k.0((), ())\n}\n".to_string(),
            "a tail resume is formed before contract in `f`",
        ),
        (
            format!(
                "effect Ask {{ ask/1 }}\nfn f() -> tobj {{\n  tail handle Ask((), &body) {{ ask: &clause }} return &ret\n}}\n{handler}"
            ),
            "a tail handler is formed before contract in `f`",
        ),
    ] {
        assert_eq!(check_translated(&text), Err(message.to_string()));
        assert_eq!(check_scopes(&text), Ok(()));
    }
}

#[test]
fn the_translated_level_rejects_boxes_and_unboxes() {
    // `box` と `unbox` は box の挿入だけが入れる
    assert_eq!(
        check_translated(&boxed_round_trip("")),
        Err("`n.0` is boxed before the boxing pass in `f`".to_string())
    );
    assert_eq!(
        check_translated("fn f() -> tobj {\n  let b.0: tobj = box 5\n  return b.0\n}\n"),
        Err("5 is boxed before the boxing pass in `f`".to_string())
    );
    assert_eq!(
        check_translated("fn f(b.0: tobj) -> int {\n  let n.1: int = unbox b.0\n  return n.1\n}\n"),
        Err("`b.0` is unboxed before the boxing pass in `f`".to_string())
    );
}

#[test]
fn the_translated_level_rejects_what_perceus_inserts() {
    let dup = "fn twice(s.0: obj) -> obj {\n  dup s.0\n  let t.1: obj = extern Prelude.++(s.0, s.0)\n  return t.1\n}\n";
    assert_eq!(
        check_translated(dup),
        Err("`s.0` is duplicated before Perceus in `twice`".to_string())
    );
    let saved = format!(
        "{IDENTITY}fn f(s.0: obj) -> obj {{\n  let t.1: obj = call g(s.0) save [s.0]\n  return t.1\n}}\n"
    );
    assert_eq!(
        check_translated(&saved),
        Err("a call saves [s.0] before Perceus in `f`".to_string())
    );
}

// box と unbox

/// `n` と `c` と定数を `box` し、`unbox` で戻す。`decrefs` は、所有の段で `box` の変数を手放す文である。
fn boxed_round_trip(decrefs: &str) -> String {
    format!(
        "fn f(n.0: int, c.1: enum) -> int {{
  let b.2: tobj = box n.0
  let e.3: tobj = box c.1
  let k.4: tobj = box 5
  let m.5: int = unbox b.2
  let d.6: enum = unbox e.3
  let o.7: int = unbox k.4
{decrefs}  return m.5
}}
"
    )
}

#[test]
fn int_and_enum_values_and_int_constants_are_boxed_and_unboxed() {
    assert_eq!(check_scopes(&boxed_round_trip("")), Ok(()));
    assert_eq!(
        check(&boxed_round_trip(
            "  decref b.2\n  decref e.3\n  decref k.4\n"
        )),
        Ok(())
    );
    // `box` は所有した `tobj` を作る
    assert_eq!(
        check(&boxed_round_trip("")),
        Err("`b.2` is still owned at the end of the function in `f`".to_string())
    );
}

#[test]
fn only_int_and_enum_values_and_int_constants_are_boxed() {
    for (operand, shown) in [
        ("u.0", "`u.0` (unit)"),
        ("p.1", "`p.1` (obj)"),
        ("t.2", "`t.2` (tobj)"),
        ("()", "()"),
        ("#1", "#1"),
        ("&f", "&f"),
    ] {
        let text = format!(
            "fn f(u.0: unit, p.1: obj, t.2: tobj) -> tobj {{\n  let b.3: tobj = box {operand}\n  return b.3\n}}\n"
        );
        let expected = Err(format!(
            "{shown} is boxed, but only int and enum values and Int constants can be in `f`"
        ));
        assert_eq!(check_scopes(&text), expected, "{operand}");
        assert_eq!(check(&text), expected, "{operand}");
    }
}

#[test]
fn a_box_is_bound_to_tobj() {
    for repr in ["int", "obj", "unit"] {
        let text =
            format!("fn f(n.0: int) -> {repr} {{\n  let b.1: {repr} = box n.0\n  return b.1\n}}\n");
        assert_eq!(
            check_scopes(&text),
            Err(format!(
                "`b.1` ({repr}) is bound to a box, which is tobj in `f`"
            )),
            "{repr}"
        );
    }
}

#[test]
fn only_tobj_values_are_unboxed() {
    // `obj` はつねにヒープの物体を指すので、スカラーを入れた値にならない
    for (operand, shown) in [
        ("p.1", "`p.1` (obj)"),
        ("n.0", "`n.0` (int)"),
        ("5", "5"),
        ("()", "()"),
    ] {
        let text = format!(
            "fn f(n.0: int, p.1: obj) -> int {{\n  let m.2: int = unbox {operand}\n  return m.2\n}}\n"
        );
        let expected = Err(format!("{shown} is unboxed, but only tobj can be in `f`"));
        assert_eq!(check_scopes(&text), expected, "{operand}");
        assert_eq!(check(&text), expected, "{operand}");
    }
}

#[test]
fn an_unbox_gives_an_int_or_an_enum() {
    for repr in ["unit", "tobj", "obj"] {
        let text =
            format!("fn f(t.0: tobj) -> unit {{\n  let m.1: {repr} = unbox t.0\n  return ()\n}}\n");
        assert_eq!(
            check_scopes(&text),
            Err(format!(
                "`m.1` ({repr}) is bound to an unbox, which gives int or enum in `f`"
            )),
            "{repr}"
        );
    }
}

#[test]
fn an_unbox_reads_its_value() {
    // 読むだけなので、`unbox` の後も値を所有している。借りたフィールドも、持ち主が所有している間は読める
    assert_eq!(
        check(
            "fn f(t.0: tobj) -> int {\n  let n.1: int = unbox t.0\n  decref t.0\n  return n.1\n}\n"
        ),
        Ok(())
    );
    assert_eq!(
        check(&unpacking("  let n.3: int = unbox b.2\n  return p.0\n")),
        Ok(())
    );
    assert_eq!(
        check(
            "fn f(t.0: tobj) -> int {\n  decref t.0\n  let n.1: int = unbox t.0\n  return n.1\n}\n"
        ),
        Err("`t.0` is unboxed after it was moved in `f`".to_string())
    );
    assert_eq!(
        check(&unpacking(
            "  decref p.0\n  let n.3: int = unbox b.2\n  let e.4: obj = const \"e\"\n  return e.4\n"
        )),
        Err("`b.2` is unboxed after its owner `p.0` was given up in `f`".to_string())
    );
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
