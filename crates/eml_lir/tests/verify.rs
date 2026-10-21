//! LIR のテキストで書いた LIR で、verifier が正しいものを受け入れ、壊れたものを拒むことを確かめる (docs/spec/lir.md)。
//! 構造の規則 R1 から R8 と、前の形の IR から引き継いだ検査を1つずつ確かめる。

use eml_lir::{
    Prim, Program, Rhs, Stmt, parse, verify, verify_flattened, verify_lowered, verify_scopes,
};

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

fn check_lowered(text: &str) -> Result<(), String> {
    verify_lowered(&read(text)).map_err(|error| error.to_string())
}

/// spec の「テキストの形」の例。RC の対象の変数がないので、どちらの段でも通る。
const SPEC_EXAMPLE: &str = "\
layout Prelude.Bool { False, True }
fn f(x.0: int) -> int {
  let c.1: enum = extern \"Prelude.Ord Int.<\"(x.0, 10)
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  jump b3(x.0)
b2:
  let t.2: int = extern \"Prelude.Num Int.+\"(x.0, 1) @\"main.em\":2:20
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

/// `n` を使う合流のブロックの前で、片方の経路だけが呼び出しをする。
fn call_on_one_path() -> String {
    format!(
        "{BOOL}{K}fn f(n.0: int, c.1: enum) -> int {{
  switch c.1 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let t.2: int = call k(1)
  jump b3(t.2)
b2:
  jump b3(1)
b3(r.3: int):
  let u.4: int = extern \"Prelude.Num Int.+\"(n.0, r.3)
  return u.4
}}
"
    )
}

const ASK: &str = "effect Ask { ask(tobj) -> tobj }\n";

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
  let t.1: int = extern \"Prelude.Num Int.+\"(x.0, 1)
  jump b3()
b2:
  let t.1: int = extern \"Prelude.Num Int.+\"(x.0, 2)
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
  let a.1: int = extern \"Prelude.Num Int.+\"(b.2, 1)
  let b.2: int = extern \"Prelude.Num Int.+\"(x.0, 1)
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
    let text =
        "fn f() -> int {\n  let a.0: int = extern \"Prelude.Num Int.+\"(a.0, 1)\n  return a.0\n}\n";
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
  let t.1: int = extern \"Prelude.Num Int.+\"(x.0, 1)
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
  let t.1: int = extern \"Prelude.Num Int.+\"(x.0, 1)
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

// R6: 呼び出しの後

#[test]
fn scopes_keep_variables_in_scope_after_a_call() {
    let text = format!(
        "{K}fn f(n.0: int) -> int {{
  let t.1: int = call k(n.0)
  let t.2: int = extern \"Prelude.Num Int.+\"(n.0, t.1)
  return t.2
}}
"
    );
    assert_eq!(check_scopes(&text), Ok(()));
    assert_eq!(check_scopes(&call_on_one_path()), Ok(()));
}

// R7

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

/// 互換でない `return` は、末尾の位置の呼び出しの後でも拒む。box の挿入は末尾の位置の呼び出しの結果も関数の `ret`
/// へ変換するためである (docs/spec/lir.md の「末尾の位置の呼び出し」)。
#[test]
fn an_incompatible_return_is_rejected_even_after_a_tail_position_call() {
    let g = "fn g(x.0: tobj) -> tobj {\n  return x.0\n}\n";
    let tail = format!(
        "{g}fn f(c.0: tobj) -> int {{\n  let r.1: tobj = call g(c.0)\n  let s.2: obj = const \"unused\"\n  return r.1\n}}\n"
    );
    assert_eq!(
        check_scopes(&tail),
        Err("`r.1` (tobj) is returned from a function that returns int in `f`".to_string())
    );
    for text in [
        format!(
            "{g}fn f(c.0: tobj, s.1: obj) -> int {{\n  let r.2: tobj = call g(c.0)\n  let u.3: unit = extern Prelude.println(s.1)\n  return r.2\n}}\n"
        ),
        format!("{g}fn f(c.0: tobj) -> int {{\n  return c.0\n}}\n"),
    ] {
        let returned = if text.contains("r.2") { "r.2" } else { "c.0" };
        assert_eq!(
            check_scopes(&text),
            Err(format!(
                "`{returned}` (tobj) is returned from a function that returns int in `f`"
            ))
        );
    }
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
    // `obj` と `tobj`、`unit` と `tobj` は互換で、命令なしで行き来する (docs/spec/lir.md の「値の表現」)
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
        assert_eq!(rejected_at_both_levels(text), message);
    }
}

#[test]
fn the_uses_of_a_never_binder_are_not_compared() {
    // `never` の操作の `clause` の束縛の使いには制御が届かないので、互換の位置 (`return` の値、`jump` の実引数、
    // 直接の呼び出しの引数、`closure` の引数、`apply` のオペランド、`tobj` のフィールド) で位置と比べない。縮約は、
    // `f` の形をそのまま `tail clause` にする
    let text = "\
layout Option { None, Some(tobj) }
effect Fail { never fail(tobj) -> never }
fn f(c.0: tobj, e.1: obj) -> tobj {
  let t.2: int = clause Fail.fail c.0(e.1, ())
  return t.2
}
fn g(c.0: tobj, e.1: obj) -> tobj {
  let t.2: int = clause Fail.fail c.0(e.1, ())
  jump b1(t.2)
b1(a.3: tobj):
  return a.3
}
fn h(c.0: tobj, e.1: obj) -> tobj {
  let t.2: int = clause Fail.fail c.0(e.1, ())
  let r.3: tobj = call k(t.2)
  return r.3
}
fn k(x.0: tobj) -> tobj {
  return x.0
}
fn m(c.0: tobj, e.1: obj) -> tobj {
  let t.2: int = clause Fail.fail c.0(e.1, ())
  let c.3: tobj = closure n(t.2)
  return c.3
}
fn n(a.0: tobj, b.1: unit) -> tobj {
  return a.0
}
fn p(c.0: tobj, e.1: obj) -> tobj {
  let t.2: int = clause Fail.fail c.0(e.1, ())
  let r.3: tobj = apply &k(t.2)
  return r.3
}
fn q(c.0: tobj, e.1: obj) -> tobj {
  let t.2: int = clause Fail.fail c.0(e.1, ())
  let d.3: tobj = con Option #1(t.2)
  return d.3
}
";
    assert_eq!(check_lowered(text), Ok(()));
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
    let text = "fn f(s.0: obj) -> int {\n  let t.1: int = extern \"Prelude.Num Int.+\"(s.0, 1)\n  return t.1\n}\n";
    assert_eq!(
        rejected_at_both_levels(text),
        "argument 0 of `Prelude.Num Int.+` is `s.0` (obj), but the extern takes int in `f`"
    );
}

#[test]
fn an_extern_argument_constant_that_does_not_fit_is_rejected() {
    let text = "fn f(n.0: int) -> int {\n  let t.1: int = extern \"Prelude.Num Int.+\"(n.0, ())\n  return t.1\n}\n";
    assert_eq!(
        rejected_at_both_levels(text),
        "argument 1 of `Prelude.Num Int.+` is (), but the extern takes int in `f`"
    );
}

#[test]
fn an_extern_result_bound_to_another_repr_is_rejected() {
    let text = "fn f(n.0: int) -> obj {\n  let t.1: obj = extern \"Prelude.Ord Int.<\"(n.0, 1)\n  return t.1\n}\n";
    assert_eq!(
        rejected_at_both_levels(text),
        "`t.1` (obj) is bound to `Prelude.Ord Int.<`, which returns enum in `f`"
    );
}

#[test]
fn an_extern_takes_constants_that_fit_its_row() {
    // 引数のないコンストラクタは `enum` の引数に収まる
    let text = "fn f() -> enum {\n  let c.0: enum = extern \"Prelude.Eq Bool.==\"(#1, #0)\n  return c.0\n}\n";
    assert_eq!(check_scopes(text), Ok(()));
    assert_eq!(check(text), Ok(()));
}

// 境界の検査

/// 値として使う一様な関数。
const UNIFORM: &str = "\
fn i(u.0: unit) -> unit {
  return u.0
}
";

/// 引数が `params` で本体が `body` の `f` に、エフェクト `Ask` と `Fail` と一様な関数を添える。
fn calling(params: &str, body: &str) -> String {
    format!(
        "{ASK}effect Fail {{ never fail(tobj) -> never }}\nfn f({params}) -> tobj {{\n{body}}}\n{UNIFORM}"
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
  let u.1: unit = call h()
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
fn a_function_value_is_uniform_beyond_its_captures() {
    // `apply` は引数と結果を一様に渡すので、関数の値にする関数は捕獲より後ろの引数と結果が一様である。`&k` と `&h` は
    // そうでないので拒む (docs/spec/lir.md の「関数の ABI」)
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
    for name in ["k", "h"] {
        assert_eq!(
            check(&value(&format!("&{name}"))),
            Err(format!(
                "`{name}` is used as a value with 0 captures, but its later parameters and its result are not all uniform in `f`"
            ))
        );
    }
    // 関数の値は位置に収まるかを確かめる。`&i` は `int` の位置に収まらない
    assert_eq!(
        rejected_at_both_levels(&format!("fn f() -> int {{\n  return &i\n}}\n{UNIFORM}")),
        "&i is returned from a function that returns int in `f`"
    );
    for body in [
        "  let d.0: tobj = con Option #1(&i)\n  return d.0\n",
        "  let c.0: tobj = closure g(&i)\n  return c.0\n",
        "  let c.0: tobj = closure g(())\n  let t.1: tobj = apply c.0(&i)\n  return t.1\n",
    ] {
        let text = format!(
            "{OPTION}fn f() -> tobj {{\n{body}}}\nfn g(a.0: tobj, b.1: unit) -> tobj {{\n  return a.0\n}}\n{UNIFORM}"
        );
        assert_eq!(check(&text), Ok(()));
    }
}

#[test]
fn a_closure_checks_its_argument_scopes() {
    // 捕獲の位置は一様でなくてよい。捕獲の引数の範囲は確かめる
    let closure = |g: &str, arg: &str| {
        format!(
            "\
fn f() -> tobj {{
  let c.0: tobj = closure g({arg})
  return c.0
}}
fn g(a.0: {g}, b.1: tobj) -> tobj {{
  return b.1
}}
"
        )
    };
    assert_eq!(check(&closure("int", "5")), Ok(()));
    assert_eq!(
        rejected_at_both_levels(&closure("tobj", "c.0")),
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
            "c.0: int, e.1: obj",
            "  let t.2: tobj = clause Ask.ask c.0(e.1, ())\n  return t.2\n",
            "the callee of a clause of `Ask.ask` is `c.0` (int), but a clause takes tobj",
        ),
        (
            "c.0: tobj, e.1: obj, n.2: int",
            "  let t.3: tobj = clause Ask.ask c.0(e.1, n.2)\n  return t.3\n",
            "argument 1 of a clause of `Ask.ask` is `n.2` (int), but the operation takes tobj",
        ),
        (
            "c.0: tobj, e.1: obj",
            "  let t.2: int = clause Fail.fail c.0(e.1, 1)\n  return ()\n",
            "argument 1 of a clause of `Fail.fail` is 1, but the operation takes tobj",
        ),
        (
            "n.0: int",
            "  let u.1: unit = prim yield_cont_set(n.0)\n  return u.1\n",
            "argument 0 of `prim yield_cont_set` is `n.0` (int), but the primitive takes tobj",
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
            "c.0: tobj, e.1: obj",
            "  let t.2: int = clause Ask.ask c.0(e.1, ())\n  return ()\n",
            "`t.2` (int) is bound to a clause of `Ask.ask`, which returns tobj",
        ),
        (
            "",
            "  let w.0: int = prim evv_get()\n  return ()\n",
            "`w.0` (int) is bound to `prim evv_get`, which returns tobj",
        ),
    ] {
        assert_eq!(
            rejected_at_both_levels(&calling(params, body)),
            format!("{message} in `f`")
        );
    }
}

/// 末尾呼び出しの結果の Repr は、呼び出し元の `ret` と互換である (docs/spec/lir.md の「末尾の位置の呼び出し」)。
#[test]
fn a_tail_call_result_is_compatible_with_the_ret() {
    let tail = |ret: &str, params: &str, call: &str| {
        format!("{ASK}fn f({params}) -> {ret} {{\n  tail {call}\n}}\n{K}{UNIFORM}")
    };
    for (ret, params, call) in [
        ("tobj", "c.0: tobj", "apply c.0(())"),
        ("int", "", "call k(1)"),
        ("tobj", "c.0: tobj, e.1: obj", "clause Ask.ask c.0(e.1, ())"),
    ] {
        assert_eq!(check(&tail(ret, params, call)), Ok(()), "{call}");
    }
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
            "a tail apply returns tobj poly, but this function returns int",
        ),
        (
            "int",
            "c.0: tobj, e.1: obj",
            "clause Ask.ask c.0(e.1, ())",
            "a tail clause of `Ask.ask` returns tobj, but this function returns int",
        ),
    ] {
        assert_eq!(
            check(&tail(ret, params, call)),
            Err(format!("{message} in `f`")),
            "{call}"
        );
    }
}

// R8: データの配置

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
        // `float` には case のリテラルがないので、どのリテラルの `switch` にもならない
        (
            "x.0: float",
            "switch x.0 { 1 -> b1, _ -> b2 }",
            "`x.0` (float) is switched on Int literals",
        ),
        (
            "",
            "switch 1.5 { 1 -> b1, _ -> b2 }",
            "1.5 is switched on Int literals",
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
        "  let t.4: obj = call g(x.2)\n  return t.4\n",
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
    // (docs/spec/lir.md の「verifier」)
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

/// `p` を分解し、`body` が `p` から借りたフィールド `a` と `b` を使う。`g` は呼び出す関数である。
fn unpacking_around_a_call(body: &str) -> String {
    format!(
        "layout Pair {{ Pair(obj, obj) }}\n{IDENTITY}fn f(p.0: obj) -> obj {{\n  unpack p.0 Pair #0(a.1: obj, b.2: obj)\n{body}}}\n"
    )
}

#[test]
fn a_borrowed_field_stays_valid_across_a_call_while_its_owner_is_owned() {
    // Perceus は、持ち主が呼び出しの後でも要るなら、フィールドを借りたまま呼び出しをまたがせる
    // (docs/spec/lir.md の「所有を取る時点」)
    assert_eq!(
        check(&unpacking_around_a_call(
            "  dup a.1\n  let r.3: obj = call g(a.1)\n  decref r.3\n  dup b.2\n  let q.4: obj = con Pair #0(p.0, b.2)\n  return q.4\n"
        )),
        Ok(())
    );
}

#[test]
fn a_field_cannot_be_read_after_a_call_consumed_its_owner() {
    assert_eq!(
        check(&unpacking_around_a_call(
            "  let r.3: obj = call g(p.0)\n  dup b.2\n  let q.4: obj = con Pair #0(r.3, b.2)\n  return q.4\n"
        )),
        Err("`b.2` is duplicated after its owner `p.0` was given up in `f`".to_string())
    );
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
    let text = "fn f(a.0: int) -> int {\n  let t.1: int = extern \"Prelude.Num Int.+\"(a.0)\n  return t.1\n}\n";
    assert_eq!(
        check(text),
        Err("`Prelude.Num Int.+` takes 2 arguments but is given 1 in `f`".to_string())
    );
}

// 入口の段

#[test]
fn the_lowered_level_rejects_tail_calls() {
    // 末尾呼び出しは縮約だけが作る。box の挿入は、末尾呼び出しのない入力の `let` と `return` から末尾の位置を見る
    for (text, message) in [
        (
            "fn f(x.0: int) -> int {\n  tail call f(x.0)\n}\n",
            "a tail call is formed before contract in `f`",
        ),
        (
            "fn f(c.0: tobj) -> tobj {\n  tail apply c.0(())\n}\n",
            "a tail apply is formed before contract in `f`",
        ),
    ] {
        assert_eq!(check_lowered(text), Err(message.to_string()));
        assert_eq!(check_scopes(text), Ok(()));
    }
}

#[test]
fn the_lowered_level_rejects_boxes_and_unboxes() {
    // `box` と `unbox` は box の挿入だけが入れる
    assert_eq!(
        check_lowered(&boxed_round_trip("")),
        Err("`n.0` is boxed before the boxing pass in `f`".to_string())
    );
    assert_eq!(
        check_lowered("fn f() -> tobj {\n  let b.0: tobj = box 5\n  return b.0\n}\n"),
        Err("5 is boxed before the boxing pass in `f`".to_string())
    );
    assert_eq!(
        check_lowered("fn f(b.0: tobj) -> int {\n  let n.1: int = unbox b.0\n  return n.1\n}\n"),
        Err("`b.0` is unboxed before the boxing pass in `f`".to_string())
    );
}

#[test]
fn the_lowered_level_rejects_what_perceus_inserts() {
    let dup = "fn twice(s.0: obj) -> obj {\n  dup s.0\n  let t.1: obj = extern Prelude.++(s.0, s.0)\n  return t.1\n}\n";
    assert_eq!(
        check_lowered(dup),
        Err("`s.0` is duplicated before Perceus in `twice`".to_string())
    );
}

#[test]
fn the_lowered_level_comes_before_boxing_and_contract() {
    assert_eq!(
        check_lowered("fn f(x.0: int) -> int {\n  tail call f(x.0)\n}\n"),
        Err("a tail call is formed before contract in `f`".to_string())
    );
    assert_eq!(
        check_lowered("fn f() -> tobj {\n  let b.0: tobj = box 1\n  return b.0\n}\n"),
        Err("1 is boxed before the boxing pass in `f`".to_string())
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
fn only_boxed_scalar_values_and_int_and_float_constants_are_boxed() {
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
            "{shown} is boxed, but only int, float and enum values and Int and Float constants can be in `f`"
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
fn an_unbox_gives_a_boxed_scalar() {
    for repr in ["unit", "tobj", "obj"] {
        let text =
            format!("fn f(t.0: tobj) -> unit {{\n  let m.1: {repr} = unbox t.0\n  return ()\n}}\n");
        assert_eq!(
            check_scopes(&text),
            Err(format!(
                "`m.1` ({repr}) is bound to an unbox, which gives int, float or enum in `f`"
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

#[test]
fn a_borrowed_prim_argument_is_read_at_the_ownership_level() {
    // 借りたフィールドを借用の引数に渡せ、所有の数を変えない
    let lent = "\
layout W { W(obj) }
fn g(w.0: obj) -> tobj {
  unpack w.0 W #0(e.1: obj)
  let v.2: tobj = prim ev_evv(e.1)
  decref w.0
  return v.2
}
";
    assert_eq!(check(lent), Ok(()));
    // 持ち主を手放した後に借用の引数に渡すと拒む
    let after_owner = "\
layout W { W(obj) }
fn g(w.0: obj) -> tobj {
  unpack w.0 W #0(e.1: obj)
  decref w.0
  let v.2: tobj = prim ev_evv(e.1)
  return v.2
}
";
    assert_eq!(
        check(after_owner),
        Err("`e.1` is lent to `prim ev_evv` after its owner `w.0` was given up in `g`".to_string())
    );
    // 所有の変数を借用の引数に渡しても、所有は残る。手放さなければ漏れとして拒む
    let kept = "\
fn f(e.0: obj) -> tobj {
  let v.1: tobj = prim ev_evv(e.0)
  return v.1
}
";
    assert_eq!(
        check(kept),
        Err("`e.0` is still owned at the end of the function in `f`".to_string())
    );
}

// 大きさ

/// `n` 個の `switch` が続き、片方の枝が呼び出しをして合流する関数。所有の検査の段では、合流のブロックの数だけ、
/// 入口の所有を比べる。
fn chain_of_switches(n: u32) -> String {
    let mut text = format!(
        "{BOOL}{K}fn f(s.0: obj, n.1: int, c.2: enum) -> obj {{\n  switch c.2 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}\n"
    );
    for i in 0..n {
        let (call, other, merge) = (3 * i + 1, 3 * i + 2, 3 * i + 3);
        text.push_str(&format!(
            "b{call}:\n  let t.{}: int = call k(n.1)\n  jump b{merge}()\nb{other}:\n  jump b{merge}()\nb{merge}:\n",
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
    // ブロックごとに所有を関数の先頭から数え直すと、2乗の時間がかかる
    let program = read(&chain_of_switches(20000));
    let start = std::time::Instant::now();
    assert_eq!(verify_scopes(&program), Ok(()));
    assert_eq!(verify(&program), Ok(()));
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

// 基本操作と節の呼び出し

const STATE: &str = "effect State { get(unit) -> tobj, put(tobj) -> unit }\n";
const COUNTER: &str = "effect Counter { get(unit) -> int, put(int) -> unit }\n";

/// handle の場所で入口の表を作り、`State.get` を節の呼び出しで呼ぶ形。
const EVIDENCE_EXAMPLE: &str = "\
effect State { get(unit) -> tobj, put(tobj) -> unit }
yields fn f(s.0: tobj) -> tobj {
  let w.1: tobj = prim evv_get()
  let e.2: obj = prim ev_new[State](s.0, w.1)
  let u.3: unit = prim ev_set_clause[State.get](e.2, &g)
  let m.4: tobj = prim evv_mask[State*2](w.1)
  let c.5: tobj = prim ev_clause[State.get](e.2)
  let r.6: tobj = clause State.get c.5(e.2, ())
  return r.6
}
internal yields clause State.get fn g(e.0: obj, u.1: unit) -> tobj {
  let s.2: tobj = prim ev_take_state(e.0)
  return s.2
}
";

/// `Counter.put` の印を持ち、捕獲を1つ持つ節の関数。
const PUT_CLAUSE: &str =
    "internal clause Counter.put fn g(m.0: tobj, e.1: obj, n.2: int) -> unit {\n  return ()\n}\n";

#[test]
fn prims_and_clauses_are_accepted_after_the_evidence_pass() {
    assert_eq!(check_lowered(EVIDENCE_EXAMPLE), Ok(()));
    assert_eq!(check_scopes(EVIDENCE_EXAMPLE), Ok(()));
}

#[test]
fn the_lowered_level_checks_prims_and_clauses() {
    assert_eq!(check_lowered(EVIDENCE_EXAMPLE), Ok(()));
    assert_eq!(
        check_lowered("fn f() -> unit {\n  let u.0: unit = prim evv_set()\n  return u.0\n}\n"),
        Err("`prim evv_set` takes 1 arguments but is given 0 in `f`".to_string())
    );
}

#[test]
fn the_boundaries_of_prims_and_clauses_are_compatible() {
    let errors: Vec<String> = [
        format!(
            "{STATE}fn f(n.0: int, w.1: tobj) -> obj {{\n  let e.2: obj = prim ev_new[State](n.0, w.1)\n  return e.2\n}}\n"
        ),
        "fn f() -> int {\n  let w.0: int = prim evv_get()\n  return w.0\n}\n".to_string(),
        format!(
            "{COUNTER}fn f(c.0: tobj, n.1: tobj, e.2: obj) -> unit {{\n  let u.3: unit = clause Counter.put c.0(e.2, n.1)\n  return u.3\n}}\n"
        ),
        format!(
            "{COUNTER}fn f(c.0: int, e.1: obj) -> int {{\n  let t.2: int = clause Counter.get c.0(e.1, ())\n  return t.2\n}}\n"
        ),
        format!(
            "{COUNTER}fn f(c.0: tobj, e.1: int) -> int {{\n  let t.2: int = clause Counter.get c.0(e.1, ())\n  return t.2\n}}\n"
        ),
        format!(
            "{STATE}fn f(c.1: tobj, e.2: obj) -> int {{\n  let t.0: int = clause State.get c.1(e.2, ())\n  return t.0\n}}\n"
        ),
    ]
    .iter()
    .map(|text| check_scopes(text).unwrap_err())
    .collect();
    assert_eq!(
        errors,
        [
            "argument 0 of `prim ev_new[State]` is `n.0` (int), but the primitive takes tobj in `f`",
            "`w.0` (int) is bound to `prim evv_get`, which returns tobj in `f`",
            "argument 1 of a clause of `Counter.put` is `n.1` (tobj), but the operation takes int in `f`",
            "the callee of a clause of `Counter.get` is `c.0` (int), but a clause takes tobj in `f`",
            "argument 0 of a clause of `Counter.get` is `e.1` (int), but a clause takes obj in `f`",
            "`t.0` (int) is bound to a clause of `State.get`, which returns tobj in `f`",
        ]
    );
}

#[test]
fn a_never_clause_has_no_result_to_compare() {
    let text = "\
effect Fail { never fail(tobj) -> never }
fn f(c.0: tobj, e.1: obj) -> tobj {
  let t.2: int = clause Fail.fail c.0(e.1, ())
  return t.2
}
fn g(c.0: tobj, e.1: obj) -> int {
  tail clause Fail.fail c.0(e.1, ())
}
";
    assert_eq!(check_scopes(text), Ok(()));
    assert_eq!(check(text), Ok(()));
}

#[test]
fn a_clause_function_has_the_reprs_of_its_operation() {
    let clause = |head: &str, ret: &str| {
        let body = match ret {
            "obj" => "  let d.9: obj = prim dummy()\n  return d.9\n",
            "int" => "  return 1\n",
            _ => "  return ()\n",
        };
        check_scopes(&format!(
            "{COUNTER}effect Fail {{ never fail(tobj) -> never }}\nfn f() -> int {{\n  return 1\n}}\ninternal clause {head} -> {ret} {{\n{body}}}\n"
        ))
    };
    assert_eq!(
        clause("Counter.put fn g(m.0: tobj, e.1: obj, n.2: int)", "unit"),
        Ok(())
    );
    assert_eq!(clause("Fail.fail fn g(e.0: obj, x.1: tobj)", "obj"), Ok(()));
    assert_eq!(
        clause("Counter.put fn g(m.0: tobj, e.1: obj, n.2: tobj)", "unit"),
        Err("`g` is a clause of `Counter.put`, but its parameter 2 is tobj in `g`".to_string())
    );
    assert_eq!(
        clause("Counter.put fn g(m.0: int, e.1: obj, n.2: int)", "unit"),
        Err("`g` is a clause of `Counter.put`, but its parameter 0 is int in `g`".to_string())
    );
    assert_eq!(
        clause("Counter.put fn g(e.0: int, n.1: int)", "unit"),
        Err("`g` is a clause of `Counter.put`, but its parameter 0 is int in `g`".to_string())
    );
    assert_eq!(
        clause("Counter.get fn g(e.0: obj, u.1: unit)", "tobj"),
        Err("`g` is a clause of `Counter.get`, but it returns tobj in `g`".to_string())
    );
    assert_eq!(
        clause("Fail.fail fn g(e.0: obj, x.1: tobj)", "int"),
        Err("`g` is a clause of `Fail.fail`, but it returns int in `g`".to_string())
    );
    // 引数の数は境界の検査でないので、入口の段でも確かめる
    assert_eq!(
        check_lowered(&format!(
            "{COUNTER}fn f() -> int {{\n  return 1\n}}\ninternal clause Counter.put fn g(n.0: int) -> unit {{\n  return ()\n}}\n"
        )),
        Err("`g` is a clause of `Counter.put`, so it takes the evidence and 1 arguments after its captures, but it has 1 parameters in `g`".to_string())
    );
    assert_eq!(
        check_lowered(&format!(
            "{COUNTER}fn f() -> int {{\n  return 1\n}}\ninternal clause Counter.#4 fn g(e.0: obj) -> unit {{\n  return ()\n}}\n"
        )),
        Err(
            "`g` is a clause of operation 4 of `Counter`, which has 2 operations in `g`"
                .to_string()
        )
    );
}

#[test]
fn a_clause_function_is_only_put_into_a_handler_table() {
    let used_as_value = Err(
        "`g` is a clause of `Counter.put`, but it is used as a function value in `f`".to_string(),
    );
    assert_eq!(
        check_lowered(&format!(
            "{COUNTER}fn f(c.0: tobj) -> tobj {{\n  let t.1: tobj = apply c.0(&g)\n  return t.1\n}}\n{PUT_CLAUSE}"
        )),
        used_as_value
    );
    assert_eq!(
        check_lowered(&format!(
            "{COUNTER}fn f(m.0: tobj, c.1: tobj) -> tobj {{\n  let g.2: tobj = closure g(m.0)\n  let t.3: tobj = apply c.1(g.2)\n  return t.3\n}}\n{PUT_CLAUSE}"
        )),
        used_as_value
    );
    assert_eq!(
        check_lowered(&format!(
            "{COUNTER}fn f() -> tobj {{\n  return &g\n}}\n{PUT_CLAUSE}"
        )),
        used_as_value
    );
    assert_eq!(
        check_lowered(&format!(
            "{COUNTER}fn f(m.0: tobj, e.1: obj) -> unit {{\n  let u.2: unit = call g(m.0, e.1, 1)\n  return u.2\n}}\n{PUT_CLAUSE}"
        )),
        Err("`g` is a clause of `Counter.put`, but it is called directly in `f`".to_string())
    );
    // 捕獲のある節は、同じ関数の中の `closure` で入口の表に入れる
    let put = format!(
        "{COUNTER}fn f(m.0: tobj, w.1: tobj) -> unit {{\n  let e.2: obj = prim ev_new[Counter]((), w.1)\n  let g.3: tobj = closure g(m.0)\n  let u.4: unit = prim ev_set_clause[Counter.put](e.2, g.3)\n  return u.4\n}}\n{PUT_CLAUSE}"
    );
    assert_eq!(check_lowered(&put), Ok(()));
    assert_eq!(check_scopes(&put), Ok(()));
}

#[test]
fn a_prim_names_known_effects_and_operations() {
    assert_eq!(
        check_lowered("fn f() -> obj {\n  let e.0: obj = prim ev_lookup[#2]()\n  return e.0\n}\n"),
        Err(
            "`prim ev_lookup[#2]` names effect 2, which is not in the effect table in `f`"
                .to_string()
        )
    );
    assert_eq!(
        check_lowered(&format!(
            "{STATE}fn f(e.0: obj) -> tobj {{\n  let c.1: tobj = prim ev_clause[State.#2](e.0)\n  return c.1\n}}\n"
        )),
        Err(
            "`prim ev_clause[State.#2]` names operation 2 of `State`, which has 2 operations in `f`"
                .to_string()
        )
    );
}

/// `evv_mask` の項目の並びは、呼び出しの `mask` と同じく空でなく、表にあるエフェクトを昇順に並べる。テキストは空の
/// 並びを読まないので、読んだ後で空にする。
#[test]
fn an_evv_mask_names_known_effects_in_order() {
    let mask = |effects: &str| {
        format!(
            "{STATE}{COUNTER}fn f(w.0: tobj) -> tobj {{\n  let m.1: tobj = prim evv_mask[{effects}](w.0)\n  return m.1\n}}\n"
        )
    };
    assert_eq!(check_lowered(&mask("State*2, Counter")), Ok(()));
    assert_eq!(
        check_lowered(&mask("Counter, State")),
        Err(
            "the effects of `prim evv_mask[Counter, State]` are not in ascending order in `f`"
                .to_string()
        )
    );
    assert_eq!(
        check_lowered(&mask("State, #4")),
        Err(
            "`prim evv_mask[State, #4]` names effect 4, which is not in the effect table in `f`"
                .to_string()
        )
    );
    let mut program = read(&mask("State"));
    let Stmt::Let {
        var: _,
        rhs: Rhs::Prim(Prim::EvvMask(effects), _),
    } = &mut program.functions[0].blocks[0].stmts[0]
    else {
        panic!("not an evv_mask");
    };
    effects.clear();
    assert_eq!(
        verify_lowered(&program).unwrap_err().message,
        "`prim evv_mask[]` masks no effect"
    );
}

/// 同じエフェクトの項目が2つあると、並びは真に昇順でない。
#[test]
fn an_evv_mask_names_each_effect_once() {
    assert_eq!(
        check_lowered(&format!(
            "{STATE}fn f(w.0: tobj) -> tobj {{\n  let m.1: tobj = prim evv_mask[State, State@1](w.0)\n  return m.1\n}}\n"
        )),
        Err(
            "the effects of `prim evv_mask[State, State@1]` are not in ascending order in `f`"
                .to_string()
        )
    );
}

/// 項目は 1 つ以上の handler を飛ばす。テキストは `*0` を読まないので、読んだ後で 0 にする。
#[test]
fn an_evv_mask_item_skips_some_handler() {
    let mut program = read(&format!(
        "{STATE}fn f(w.0: tobj) -> tobj {{\n  let m.1: tobj = prim evv_mask[State](w.0)\n  return m.1\n}}\n"
    ));
    let Stmt::Let {
        var: _,
        rhs: Rhs::Prim(Prim::EvvMask(items), _),
    } = &mut program.functions[0].blocks[0].stmts[0]
    else {
        panic!("not an evv_mask");
    };
    items[0].skip = 0;
    assert_eq!(
        verify_lowered(&program).unwrap_err().message,
        "an item of `prim evv_mask[State*0]` skips no handler"
    );
}

/// 表にないエフェクトの操作の印は、操作の番号より先にエフェクトを報告する。
#[test]
fn a_clause_function_names_a_known_effect() {
    assert_eq!(
        check_lowered(&format!(
            "{COUNTER}fn f() -> int {{\n  return 1\n}}\ninternal clause #3.#0 fn g(e.0: obj) -> unit {{\n  return ()\n}}\n"
        )),
        Err("`g` is a clause of effect 3, which is not in the effect table in `g`".to_string())
    );
}

#[test]
fn a_clause_call_names_an_operation_and_takes_its_arguments() {
    assert_eq!(
        check_lowered(&format!(
            "{COUNTER}fn f(c.0: tobj, e.1: obj) -> unit {{\n  let u.2: unit = clause Counter.#5 c.0(e.1, 1)\n  return u.2\n}}\n"
        )),
        Err("a clause names operation 5 of `Counter`, which has 2 operations in `f`".to_string())
    );
    assert_eq!(
        check_lowered(&format!(
            "{COUNTER}fn f(c.0: tobj, e.1: obj) -> unit {{\n  let u.2: unit = clause Counter.put c.0(e.1)\n  return u.2\n}}\n"
        )),
        Err("a clause of `Counter.put` is given 1 arguments, but it takes 2 (the evidence and 1 arguments of the operation) in `f`".to_string())
    );
}

#[test]
fn ev_set_clause_names_the_clause_of_its_function() {
    let set = |entry: &str, value: &str| {
        check_lowered(&format!(
            "{STATE}{COUNTER}fn f(s.0: tobj, w.1: tobj) -> unit {{\n  let e.2: obj = prim ev_new[State](s.0, w.1)\n  let u.3: unit = prim ev_set_clause[{entry}](e.2, {value})\n  return u.3\n}}\ninternal clause Counter.put fn g(e.0: obj, n.1: int) -> unit {{\n  return ()\n}}\nfn h(e.0: obj, u.1: unit) -> tobj {{\n  return ()\n}}\n"
        ))
    };
    assert_eq!(
        set("State.get", "&g"),
        Err(
            "`g` is a clause of `Counter.put`, but it is put into the `State.get` entry in `f`"
                .to_string()
        )
    );
    assert_eq!(
        set("State.get", "&h"),
        Err("`h` is not a clause, but it is put into the `State.get` entry in `f`".to_string())
    );
    assert_eq!(
        set("Counter.put", "&g"),
        Err(
            "`prim ev_set_clause[Counter.put]` is given `e.2`, the evidence of `State` in `f`"
                .to_string()
        )
    );
}

#[test]
fn the_lowered_level_does_not_compare_boundaries() {
    // box の挿入の前なので、Repr の食い違いは box の挿入が変換で埋める
    let text = format!(
        "{COUNTER}fn f(c.0: tobj, n.1: tobj, e.2: obj) -> tobj {{\n  let u.3: unit = clause Counter.put c.0(e.2, n.1)\n  return u.3\n}}\n"
    );
    assert_eq!(check_lowered(&text), Ok(()));
}

// `passes_until` は入力の検査をデバッグビルドだけでかける (docs/spec/lir.md の「パスの順番」)。release で流す
// テスト (`nix build`) では検査がないので、このテストも走らせない
#[cfg(debug_assertions)]
#[test]
#[should_panic(
    expected = "internal error: invalid LIR before boxing: `s.0` is duplicated before Perceus in `twice`"
)]
fn passes_until_checks_its_input() {
    let mut program = read(
        "fn twice(s.0: obj) -> obj {\n  dup s.0\n  let t.1: obj = extern Prelude.++(s.0, s.0)\n  return t.1\n}\n",
    );
    eml_lir::passes_until(&mut program, eml_lir::Pass::Perceus);
}

// 印: V1、V2、作る所の検査 (docs/spec/lir.md の「Kind の印の検査」)

/// `File` を受けて閉じる関数と、`File` を返す関数。
const TAKES_LIN: &str =
    "fn g(h.0: obj lin) -> unit {\n  let u.1: unit = extern Std.Fs.close(h.0)\n  return u.1\n}\n";
const GIVES_LIN: &str =
    "fn h(s.0: obj) -> obj lin {\n  let f.1: obj lin = extern Std.Fs.open(s.0)\n  return f.1\n}\n";

/// `File` を受けて返す操作と、`unr` の値を受けて返す操作。
const LIN_OPS: &str = "effect L { take(tobj lin) -> tobj lin, ask(unit) -> tobj }\n";

#[test]
fn v1_rejects_a_dup_or_decref_of_a_lin_or_poly_variable() {
    for mark in ["lin", "poly"] {
        let dup = format!(
            "fn f(s.0: obj {mark}) -> obj {mark} {{\n  dup s.0\n  decref s.0\n  return s.0\n}}\n"
        );
        assert_eq!(
            check(&dup),
            Err(format!(
                "`s.0` ({mark}) is duplicated, but only unr can be in `f`"
            ))
        );
        let decref = format!("fn f(s.0: obj {mark}) -> unit {{\n  decref s.0\n  return ()\n}}\n");
        assert_eq!(
            check(&decref),
            Err(format!(
                "`s.0` ({mark}) is released, but only unr can be in `f`"
            ))
        );
    }
    // `release` は分解した持ち主を手放す命令で、`lin` の値にも置く
    let release = "\
layout (,) { (,)(tobj, tobj) }
fn f(p.0: obj lin) -> tobj lin {
  unpack p.0 (,) #0(a.1: tobj lin, b.2: tobj)
  release p.0 (,) #0(a.1, _)
  return a.1
}
";
    assert_eq!(check(release), Ok(()));
}

const TAKES_UNR: &str = "fn g(h.0: obj) -> unit {\n  return ()\n}\n";
const GIVES_UNR: &str = "fn h(s.0: obj) -> obj {\n  return s.0\n}\n";

#[test]
fn v2_rejects_passing_lin_to_unr_at_each_boundary() {
    for (text, message) in [
        (
            "fn f(s.0: obj lin) -> obj {\n  jump b1(s.0)\nb1(t.1: obj):\n  return t.1\n}\n"
                .to_string(),
            "a jump to b1 passes `s.0` (lin) to `t.1` (unr)",
        ),
        (
            "fn f(s.0: obj lin) -> obj {\n  return s.0\n}\n".to_string(),
            "`s.0` (lin) is returned from a function that returns unr",
        ),
        (
            format!("fn f(s.0: obj lin) -> unit {{\n  let u.1: unit = call g(s.0)\n  return u.1\n}}\n{TAKES_UNR}"),
            "argument 0 of `g` is `s.0` (lin), but the function takes unr",
        ),
        (
            format!("fn f(s.0: obj) -> obj {{\n  let t.1: obj = call h(s.0)\n  return t.1\n}}\n{GIVES_LIN}"),
            "`t.1` (unr) is bound to `h`, which returns lin",
        ),
        (
            format!("fn f(s.0: obj) -> obj {{\n  tail call h(s.0)\n}}\n{GIVES_LIN}"),
            "a tail call to `h` returns lin, but this function returns unr",
        ),
        (
            "fn f(s.0: obj lin) -> tobj {\n  let c.1: tobj = closure k(s.0)\n  return c.1\n}\nfn k(a.0: obj, b.1: tobj) -> tobj {\n  return a.0\n}\n"
                .to_string(),
            "argument 0 of a closure of `k` is `s.0` (lin), but the function takes unr",
        ),
        (
            "fn f(s.0: obj lin) -> unit {\n  let u.1: unit = extern Std.Fs.open(s.0)\n  return u.1\n}\n"
                .to_string(),
            "argument 0 of `Std.Fs.open` is `s.0` (lin), but the extern takes unr",
        ),
        (
            "fn f(s.0: obj) -> obj {\n  let h.1: obj = extern Std.Fs.open(s.0)\n  return h.1\n}\n"
                .to_string(),
            "`h.1` (unr) is bound to `Std.Fs.open`, which returns lin",
        ),
        (
            "fn f(w.0: tobj lin) -> unit {\n  let u.1: unit = prim evv_set(w.0)\n  return u.1\n}\n"
                .to_string(),
            "argument 0 of `prim evv_set` is `w.0` (lin), but the primitive takes unr",
        ),
        (
            format!("{LIN_OPS}fn f(c.0: tobj lin, e.1: obj) -> tobj {{\n  let t.2: tobj = clause L.ask c.0(e.1, ())\n  return t.2\n}}\n"),
            "the callee of a clause of `L.ask` is `c.0` (lin), but a clause takes unr",
        ),
        (
            format!("{LIN_OPS}fn f(c.0: tobj, e.1: obj lin) -> tobj {{\n  let t.2: tobj = clause L.ask c.0(e.1, ())\n  return t.2\n}}\n"),
            "argument 0 of a clause of `L.ask` is `e.1` (lin), but a clause takes unr",
        ),
        (
            format!("{LIN_OPS}fn f(c.0: tobj, e.1: obj, x.2: tobj lin) -> tobj {{\n  let t.3: tobj = clause L.take c.0(e.1, x.2)\n  return t.3\n}}\n"),
            "`t.3` (unr) is bound to a clause of `L.take`, which returns lin",
        ),
        (
            format!("{LIN_OPS}fn f(c.0: tobj, e.1: obj, x.2: tobj lin) -> tobj {{\n  tail clause L.take c.0(e.1, x.2)\n}}\n"),
            "a tail clause of `L.take` returns lin, but this function returns unr",
        ),
    ] {
        assert_eq!(rejected_at_both_levels(&text), format!("{message} in `f`"));
    }
    // `clause` の印の付いた関数は、操作の表の印で呼ばれる
    let text = format!(
        "{LIN_OPS}internal clause L.take fn g(e.0: obj, x.1: tobj) -> tobj lin {{\n  return #0\n}}\n"
    );
    assert_eq!(
        rejected_at_both_levels(&text),
        "parameter 1 of `g`, a clause of `L.take`, is `x.1` (unr), but the operation takes lin in `g`"
    );
    // 結果は操作の結果の印へ渡る
    let text = format!(
        "{LIN_OPS}internal clause L.ask fn g(e.0: obj, x.1: unit) -> tobj lin {{\n  return #0\n}}\n"
    );
    assert_eq!(
        rejected_at_both_levels(&text),
        "`g`, a clause of `L.ask`, returns lin, but the operation returns unr in `g`"
    );
}

#[test]
fn v2_lets_unr_pass_to_lin_at_each_boundary() {
    for text in [
        "fn f(s.0: obj) -> obj lin {\n  jump b1(s.0)\nb1(t.1: obj lin):\n  return t.1\n}\n".to_string(),
        "fn f(s.0: obj) -> obj lin {\n  return s.0\n}\n".to_string(),
        format!("fn f(s.0: obj) -> unit {{\n  let u.1: unit = call g(s.0)\n  return u.1\n}}\n{TAKES_LIN}"),
        format!("fn f(s.0: obj) -> obj lin {{\n  let t.1: obj lin = call h(s.0)\n  return t.1\n}}\n{GIVES_UNR}"),
        format!("fn f(s.0: obj) -> obj lin {{\n  tail call h(s.0)\n}}\n{GIVES_UNR}"),
        "fn f(s.0: obj) -> tobj lin {\n  let c.1: tobj lin = closure k(s.0)\n  return c.1\n}\nfn k(a.0: obj lin, b.1: unit) -> tobj lin {\n  return a.0\n}\n".to_string(),
        "fn f(s.0: obj) -> unit {\n  let u.1: unit = extern Std.Fs.close(s.0)\n  return u.1\n}\n".to_string(),
        "fn f() -> tobj lin {\n  let w.0: tobj lin = prim evv_get()\n  return w.0\n}\n".to_string(),
        format!("{LIN_OPS}fn f(c.0: tobj, e.1: obj, x.2: tobj) -> tobj lin {{\n  let t.3: tobj lin = clause L.take c.0(e.1, x.2)\n  return t.3\n}}\n"),
    ] {
        assert_eq!(check_scopes(&text), Ok(()), "{text}");
        assert_eq!(check(&text), Ok(()), "{text}");
    }
    // `clause` の印の付いた関数の evidence の引数は、`unr` の送り手から `lin` の受け手へ渡せる
    let text = format!(
        "{LIN_OPS}internal clause L.take fn g(e.0: obj lin, x.1: tobj lin) -> tobj lin {{\n  return x.1\n}}\n"
    );
    assert_eq!(check_scopes(&text), Ok(()), "{text}");
}

#[test]
fn v2_lets_poly_meet_unr_and_lin_and_constants_meet_any_mark() {
    let text = format!(
        "\
{OPTION}{LIN_OPS}effect Fail {{ never fail(tobj) -> never }}
fn f(s.0: obj, h.1: obj lin, c.2: tobj, e.3: obj) -> tobj lin {{
  jump b1(s.0, h.1)
b1(p.4: obj poly, q.5: obj poly):
  let u.6: unit = call g(q.5)
  let m.8: tobj lin = clause L.take c.2(e.3, #0)
  let v.9: tobj lin = clause L.take c.2(e.3, ())
  let w.10: tobj lin = clause L.take c.2(e.3, &i)
  let r.11: tobj poly = call k(p.4)
  return r.11
}}
fn k(a.0: obj) -> tobj lin {{
  let t.1: tobj = clause Fail.fail a.0(a.0, ())
  return t.1
}}
{TAKES_LIN}{UNIFORM}"
    );
    assert_eq!(check_scopes(&text), Ok(()));
    // 定数は `lin` の受け手に渡せる。`Option (List File)` の `None` を `lin` の引数に渡す形である
    let constants = format!(
        "{OPTION}fn f() -> tobj lin {{\n  jump b1(#0, 1, ())\nb1(a.0: tobj lin, b.1: int lin, c.2: unit lin):\n  return a.0\n}}\n"
    );
    assert_eq!(check(&constants), Ok(()));
}

#[test]
fn the_build_check_rejects_a_con_or_closure_below_the_join_of_its_arguments() {
    let con = |bound: &str| {
        format!(
            "{PAIR}fn f(h.0: obj lin, x.1: tobj poly) -> obj lin {{\n  let p.2: obj{bound} = con (,) #0(h.0, x.1)\n  return p.2\n}}\n"
        )
    };
    assert_eq!(check_scopes(&con(" lin")), Ok(()));
    for bound in ["", " poly"] {
        let mark = bound.trim();
        let mark = if mark.is_empty() { "unr" } else { mark };
        assert_eq!(
            check_lowered(&con(bound)),
            Err(format!(
                "`p.2` ({mark}) is bound to a con of `(,)` #0, whose arguments join to lin in `f`"
            ))
        );
    }
    // 定数は `unr` として結ぶ
    let constants = format!(
        "{OPTION}fn f() -> tobj {{\n  let o.0: tobj = con Option #1(#0)\n  return o.0\n}}\n"
    );
    assert_eq!(check(&constants), Ok(()));
    let closure = |bound: &str| {
        format!(
            "fn f(x.0: tobj poly) -> tobj poly {{\n  let c.1: tobj{bound} = closure k(x.0)\n  return c.1\n}}\nfn k(a.0: tobj poly, b.1: tobj) -> tobj poly {{\n  return a.0\n}}\n"
        )
    };
    assert_eq!(check_scopes(&closure(" poly")), Ok(()));
    assert_eq!(check_scopes(&closure(" lin")), Ok(()));
    assert_eq!(
        check_lowered(&closure("")),
        Err(
            "`c.1` (unr) is bound to a closure of `k`, whose captures join to poly in `f`"
                .to_string()
        )
    );
}

#[test]
fn v2_runs_only_on_jump_and_return_at_the_lowered_level() {
    // box の挿入の前は、境界の段と同じく呼び出しの引数を比べない
    let call = format!(
        "fn f(s.0: obj) -> unit {{\n  let u.1: unit = call g(s.0)\n  return u.1\n}}\n{TAKES_LIN}"
    );
    assert_eq!(check_lowered(&call), Ok(()));
    let extern_arg =
        "fn f(s.0: obj) -> unit {\n  let u.1: unit = extern Std.Fs.close(s.0)\n  return u.1\n}\n";
    assert_eq!(check_lowered(extern_arg), Ok(()));
    assert_eq!(
        check_lowered(
            "fn f(s.0: obj lin) -> obj {\n  jump b1(s.0)\nb1(t.1: obj):\n  return t.1\n}\n"
        ),
        Err("a jump to b1 passes `s.0` (lin) to `t.1` (unr) in `f`".to_string())
    );
    assert_eq!(
        check_lowered("fn f(s.0: obj lin) -> obj {\n  return s.0\n}\n"),
        Err("`s.0` (lin) is returned from a function that returns unr in `f`".to_string())
    );
}

/// 根の基本操作は表の行のとおりに `tobj` を出し入れする。`root_set` は値を消費し、`root_get` の結果は持ち主が手放す。
#[test]
fn root_primitives_move_tobj_values() {
    let text = "fn f(x.0: tobj) -> unit {\n  let u.1: unit = prim root_set[0](x.0)\n  let y.2: tobj = prim root_get[0]()\n  decref y.2\n  return u.1\n}\n";
    assert_eq!(check(text), Ok(()));
    assert_eq!(
        rejected_at_both_levels(
            "fn f(n.0: int) -> unit {\n  let u.1: unit = prim root_set[0](n.0)\n  return u.1\n}\n"
        ),
        "argument 0 of `prim root_set[0]` is `n.0` (int), but the primitive takes tobj in `f`"
    );
    assert_eq!(
        rejected_at_both_levels(
            "fn f() -> unit {\n  let n.0: int = prim root_get[0]()\n  return ()\n}\n"
        ),
        "`n.0` (int) is bound to `prim root_get[0]`, which returns tobj in `f`"
    );
}

// 着地の印 `prompt v`

/// `ev` を呼び出しの後も持ち続けて返す、印の付いた `apply`。
fn marked(body: &str, ev: &str) -> String {
    format!("fn f(b.0: tobj, {ev}) -> obj {{\n{body}}}\n")
}

#[test]
fn prompt_marker_is_accepted() {
    let text = marked(
        "  let x.2: tobj = yields apply b.0(()) prompt ev.1\n  decref x.2\n  return ev.1\n",
        "ev.1: obj",
    );
    assert_eq!(check(&text), Ok(()));
}

#[test]
fn prompt_marker_in_tail_position_is_rejected() {
    let text = "fn f(b.0: tobj, ev.1: obj) -> tobj {\n  tail yields apply b.0(()) prompt ev.1\n}\n";
    assert_eq!(
        check_scopes(text),
        Err(
            "a tail apply has a prompt marker, but a tail call leaves no frame to land on in `f`"
                .to_string()
        )
    );
}

#[test]
fn prompt_marker_on_a_dead_variable_is_rejected() {
    let text = marked(
        "  let x.2: tobj = yields apply b.0(()) prompt ev.1\n  return ()\n",
        "ev.1: obj",
    )
    .replace("-> obj", "-> unit");
    assert_eq!(
        check_scopes(&text),
        Err(
            "`ev.1` is the prompt marker of an apply, but it is not used after the call in `f`"
                .to_string()
        )
    );
}

#[test]
fn prompt_marker_on_a_non_obj_variable_is_rejected() {
    let text = "fn f(b.0: tobj, ev.1: tobj) -> tobj {\n  let x.2: tobj = yields apply b.0(()) prompt ev.1\n  decref x.2\n  return ev.1\n}\n";
    assert_eq!(
        check_scopes(text),
        Err("`ev.1` is the prompt marker of an apply, but it is tobj, not obj in `f`".to_string())
    );
}

#[test]
fn prompt_marker_on_an_unowned_variable_is_rejected() {
    let text = marked(
        "  decref ev.1\n  let x.2: tobj = yields apply b.0(()) prompt ev.1\n  decref x.2\n  return ev.1\n",
        "ev.1: obj",
    );
    assert_eq!(
        check(&text),
        Err(
            "`ev.1` is the prompt marker of an apply, but it is not owned after the call in `f`"
                .to_string()
        )
    );
}

#[test]
fn prompt_marker_used_only_in_a_sibling_branch_is_rejected() {
    let text = format!(
        "{BOOL}fn f(b.0: tobj, ev.1: obj, c.2: enum, e.3: obj) -> obj {{
  switch c.2 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let x.4: tobj = yields apply b.0(()) prompt ev.1
  return e.3
b2:
  return ev.1
}}
"
    );
    assert_eq!(
        check_scopes(&text),
        Err(
            "`ev.1` is the prompt marker of an apply, but it is not used after the call in `f`"
                .to_string()
        )
    );
}

#[test]
fn prompt_marker_passed_only_to_a_jump_is_accepted() {
    let text = "fn f(b.0: tobj, ev.1: obj) -> obj {
  let x.2: tobj = yields apply b.0(()) prompt ev.1
  jump b1(ev.1)
b1(p.3: obj):
  return p.3
}
";
    assert_eq!(check_scopes(text), Ok(()));
}

const P2: &str = "layout P { P(obj, obj) }\n";

/// `x` を分解して token `r` にし、`c` で分かれた行き先の b1 と b2 で、それぞれ `left` と `right` が使う
/// (docs/spec/lir.md の「verifier」)。
fn token_paths(left: &str, right: &str) -> String {
    format!(
        "{BOOL}{P2}fn f(x.0: obj, c.1: enum) -> obj {{
  unpack x.0 P #0(a.2: obj, b.3: obj)
  let r.4: token = release x.0 P #0(a.2, b.3)
  switch c.1 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}
b1:
{left}b2:
{right}}}
"
    )
}

const REBUILT: &str = "  let d.5: obj = con P #0(=a.2, b.3) reuse r.4 in place\n  return d.5\n";
const DROPPED: &str = "  drop_token r.4\n  let d.6: obj = con P #0(a.2, b.3)\n  return d.6\n";

#[test]
fn a_token_used_on_every_path_verifies() {
    assert_eq!(check(&token_paths(REBUILT, DROPPED)), Ok(()));
}

#[test]
fn token_instructions_are_rejected_before_perceus() {
    let text = token_paths(REBUILT, DROPPED);
    assert_eq!(
        check_scopes(&text),
        Err("`x.0` is released with its fields before Perceus in `f`".to_string())
    );
    assert_eq!(
        check_lowered(&text),
        Err("`x.0` is released with its fields before Perceus in `f`".to_string())
    );
}

#[test]
fn a_token_used_twice_is_rejected() {
    let left = "  let d.5: obj = con P #0(a.2, b.3) reuse r.4\n  drop_token r.4\n  return d.5\n";
    assert_eq!(
        check(&token_paths(left, DROPPED)),
        Err("the token `r.4` is used twice in `f`".to_string())
    );
}

#[test]
fn a_token_unused_on_a_path_is_rejected() {
    let right = "  let d.6: obj = con P #0(a.2, b.3)\n  return d.6\n";
    assert_eq!(
        check(&token_paths(REBUILT, right)),
        Err("the token `r.4` is still live at a return in `f`".to_string())
    );
}

#[test]
fn a_token_passed_to_a_call_is_rejected() {
    let right = "  let z.6: obj = call g(r.4)\n  drop_token r.4\n  decref z.6\n  let d.7: obj = con P #0(a.2, b.3)\n  return d.7\n";
    let text = format!("{}{IDENTITY}", token_paths(REBUILT, right));
    assert_eq!(
        check(&text),
        Err("the token `r.4` is used as a value in `f`".to_string())
    );
}

#[test]
fn a_token_passed_to_a_jump_is_rejected() {
    let text = format!(
        "{P2}fn f(x.0: obj) -> obj {{
  unpack x.0 P #0(a.1: obj, b.2: obj)
  let r.3: token = release x.0 P #0(a.1, b.2)
  jump b1(r.3)
b1(t.4: obj):
  let d.5: obj = con P #0(a.1, b.2)
  return d.5
}}
"
    );
    assert_eq!(
        check(&text),
        Err("the token `r.3` is used as a value in `f`".to_string())
    );
}

#[test]
fn a_token_live_at_a_jump_is_rejected() {
    let text = format!(
        "{P2}fn f(x.0: obj) -> obj {{
  unpack x.0 P #0(a.1: obj, b.2: obj)
  let r.3: token = release x.0 P #0(a.1, b.2)
  jump b1()
b1:
  let d.4: obj = con P #0(a.1, b.2) reuse r.3
  return d.4
}}
"
    );
    assert_eq!(
        check(&text),
        Err("the token `r.3` is still live at a jump to b1 in `f`".to_string())
    );
}

#[test]
fn a_returned_token_is_rejected() {
    let right = "  decref a.2\n  decref b.3\n  return r.4\n";
    assert_eq!(
        check(&token_paths(REBUILT, right)),
        Err("the token `r.4` is used as a value in `f`".to_string())
    );
}

#[test]
fn a_captured_token_is_rejected() {
    let right = "  let k.6: tobj = closure h(r.4)\n  drop_token r.4\n  decref k.6\n  let d.7: obj = con P #0(a.2, b.3)\n  return d.7\n";
    let h = "fn h(s.0: obj, t.1: obj) -> obj {\n  decref t.1\n  return s.0\n}\n";
    let text = format!("{}{h}", token_paths(REBUILT, right));
    assert_eq!(
        check(&text),
        Err("the token `r.4` is used as a value in `f`".to_string())
    );
}

/// `x` を token にし、`g` の呼び出しをまたいで `con` が使う。`yields` は呼ぶ側と呼ばれる側の両方の印である。
fn token_across_call(yields: &str) -> String {
    format!(
        "{P2}{yields}fn g(s.0: obj) -> obj {{
  return s.0
}}
{yields}fn f(x.0: obj) -> obj {{
  unpack x.0 P #0(a.1: obj, b.2: obj)
  let r.3: token = release x.0 P #0(a.1, b.2)
  let z.4: obj = call g(a.1)
  let d.5: obj = con P #0(z.4, b.2) reuse r.3
  return d.5
}}
"
    )
}

#[test]
fn a_token_live_across_a_yielding_call_is_rejected() {
    assert_eq!(
        check(&token_across_call("yields ")),
        Err("the token `r.3` is live after `g`, which may yield in `f`".to_string())
    );
}

#[test]
fn a_token_may_cross_a_non_yielding_call() {
    assert_eq!(check(&token_across_call("")), Ok(()));
}

#[test]
fn a_con_of_another_size_is_rejected() {
    let left = "  let d.5: obj = con Q #0(a.2, b.3, 1) reuse r.4\n  return d.5\n";
    let text = format!(
        "layout Q {{ Q(obj, obj, int) }}\n{}",
        token_paths(left, DROPPED)
    );
    assert_eq!(
        check(&text),
        Err("a con of `Q` #0 has 3 words, but the token `r.4` holds 2 words in `f`".to_string())
    );
}

#[test]
fn in_place_needs_the_same_constructor() {
    let left = "  let d.5: obj = con Q #0(a.2, b.3) reuse r.4 in place\n  return d.5\n";
    let text = format!("layout Q {{ Q(obj, obj) }}\n{}", token_paths(left, DROPPED));
    assert_eq!(
        check(&text),
        Err(
            "a con of `Q` #0 rebuilds the token `r.4` in place, but the token comes from `P` #0 in `f`"
                .to_string()
        )
    );
}

#[test]
fn in_place_needs_the_lin_condition() {
    let text = format!(
        "{P2}fn f(x.0: obj lin) -> obj {{
  unpack x.0 P #0(a.1: obj, b.2: obj)
  let r.3: token = release x.0 P #0(a.1, b.2)
  let d.4: obj poly = con P #0(b.2, a.1) reuse r.3 in place
  return d.4
}}
"
    );
    assert_eq!(
        check(&text),
        Err(
            "a con of `P` #0 rebuilds the token `r.3` in place, but `x.0` is lin and `d.4` is poly in `f`"
                .to_string()
        )
    );
}

#[test]
fn a_skip_at_another_position_is_rejected() {
    let left = "  let d.5: obj = con P #0(=b.3, a.2) reuse r.4 in place\n  return d.5\n";
    assert_eq!(
        check(&token_paths(left, DROPPED)),
        Err(
            "argument 0 of a con of `P` #0 is skipped, but `b.3` is not at the same position in the box of the token `r.4` in `f`"
                .to_string()
        )
    );
}

/// 名前のない `release` は、その道で `x` の構築子が分かっていなければならない。違う構築子で手放すと、fast mode は
/// 子を違う形で手放し、記憶がおかしくなる (docs/spec/lir.md の「verifier」)。
#[test]
fn a_token_release_of_an_unknown_constructor_is_rejected() {
    let text = format!(
        "{P2}fn f(x.0: obj, y.1: obj) -> obj {{
  dup y.1
  let r.2: token = release x.0 P #0(_, _)
  let d.3: obj = con P #0(y.1, y.1) reuse r.2
  return d.3
}}
"
    );
    assert_eq!(
        check(&text),
        Err(
            "`x.0` is released as `P` #0, but its constructor is not known on this path in `f`"
                .to_string()
        )
    );
}

/// `x` を、`c` で分かれた行き先の b1 と b2 で `left` と `right` が分解し、合流する b3 で token にする。
fn token_after_join(left: &str, right: &str) -> String {
    format!(
        "{BOOL}{P2}fn f(x.0: obj, y.1: obj, c.2: enum) -> obj {{
  switch c.2 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}
b1:
{left}  jump b3()
b2:
{right}  jump b3()
b3:
  dup y.1
  let r.7: token = release x.0 P #0(_, _)
  let d.8: obj = con P #0(y.1, y.1) reuse r.7
  return d.8
}}
"
    )
}

#[test]
fn a_token_release_known_through_a_join_verifies() {
    let left = "  unpack x.0 P #0(a.3: obj, b.4: obj)\n";
    let right = "  unpack x.0 P #0(a.5: obj, b.6: obj)\n";
    assert_eq!(check(&token_after_join(left, right)), Ok(()));
}

/// 合流するブロックで分かっている構築子は、入るすべての道で分かっているものだけである。
#[test]
fn a_token_release_known_on_one_incoming_path_is_rejected() {
    let left = "  unpack x.0 P #0(a.3: obj, b.4: obj)\n";
    assert_eq!(
        check(&token_after_join(left, "")),
        Err(
            "`x.0` is released as `P` #0, but its constructor is not known on this path in `f`"
                .to_string()
        )
    );
}

/// 入口の段でも、`tobj` でない行の位置には同じ Repr だけが渡せる。`Prelude.++` の引数は `obj` である。
#[test]
fn an_int_argument_for_an_obj_extern_parameter_is_rejected_at_the_entry_level() {
    let text = "fn f(n.0: int, s.1: obj) -> obj {
  let t.2: obj = extern Prelude.++(n.0, s.1)
  return t.2
}
";
    assert_eq!(
        check_lowered(text),
        Err(
            "argument 0 of `Prelude.++` is `n.0` (int), but the extern takes obj in `f`"
                .to_string()
        )
    );
}

/// 一様な位置の始まりは、関数の引数の数を超えない。
#[test]
fn a_split_is_not_past_the_end_of_the_parameters() {
    assert_eq!(
        check_lowered("internal uniform from 2 fn r(x.0: tobj) -> tobj {\n  return x.0\n}\n"),
        Err("uniform from 2 is past the end of the 1 parameters in `r`".to_string())
    );
}

// 構造体

const VEC3: &str = "layout Vec3 { Vec3(float, float, float) }\n";

/// 入口の段と box の挿入の後の段は構造体の変数を受ける。印だけが違う形どうしは互換である
/// (docs/spec/lir.md の「verifier」)。
#[test]
fn the_lowered_and_scopes_stages_accept_structs() {
    let text = format!(
        "{PAIR}{VEC3}fn f(v.0: Vec3{{float, float, float}}, s.1: obj) -> (,){{Vec3{{float, float, float}}, obj poly}} {{
  unpack v.0 Vec3 #0(x.2: float, y.3: float, z.4: float)
  let w.5: Vec3{{float, float, float}} = con Vec3 #0(z.4, y.3, x.2)
  let p.6: (,){{Vec3{{float, float, float}}, obj}} = con (,) #0(w.5, s.1)
  let q.7: (,){{Vec3{{float, float, float}}, obj poly}} = call g(p.6)
  return q.7
}}
fn g(p.0: (,){{Vec3{{float, float, float}}, obj poly}}) -> (,){{Vec3{{float, float, float}}, obj}} {{
  jump b1(p.0)
b1(q.1: (,){{Vec3{{float, float, float}}, obj}}):
  return q.1
}}
"
    );
    assert_eq!(check_lowered(&text), Ok(()));
    assert_eq!(check_scopes(&text), Ok(()));
    // 骨組みの違う形は互換でない
    let float = text.replace(
        "b1(q.1: (,){Vec3{float, float, float}, obj}):",
        "b1(q.1: (,){Vec3{float, float, float}, tobj}):",
    );
    assert_eq!(
        check_lowered(&float),
        Err(
            "a jump to b1 passes `p.0` ((,){Vec3{float, float, float}, obj poly}) to `q.1` ((,){Vec3{float, float, float}, tobj}) in `g`"
                .to_string()
        )
    );
}

/// 構造体の箱は配置の `con` と同じ形のヒープの物体なので、`box` の束縛は `obj` である。`unbox` は `obj` からも
/// `tobj` からも構造体を取り出す (docs/spec/lir.md の「値の表現」)。
#[test]
fn a_struct_box_is_obj() {
    let text = |boxed: &str, from: &str| {
        format!(
            "{VEC3}fn f(v.0: Vec3{{float, float, float}}, t.1: {from}) -> Vec3{{float, float, float}} {{
  let b.2: {boxed} = box v.0
  let u.3: Vec3{{float, float, float}} = unbox t.1
  return u.3
}}
"
        )
    };
    assert_eq!(check_scopes(&text("obj", "obj")), Ok(()));
    assert_eq!(check_scopes(&text("obj", "tobj")), Ok(()));
    assert_eq!(
        check_scopes(&text("tobj", "obj")),
        Err("`b.2` (tobj) is bound to a box of a struct, which is obj in `f`".to_string())
    );
}

fn check_flattened(text: &str) -> Result<(), String> {
    verify_flattened(&read(text)).map_err(|error| error.to_string())
}

/// 開いた後の段では、変数にも関数の結果にも構造体は残らない。Perceus の後の段も開いた後である
/// (docs/spec/lir.md の「verifier」)。
#[test]
fn a_struct_after_flatten_is_rejected() {
    let param = format!(
        "{VEC3}fn f(v.0: Vec3{{float, float, float}}) -> float {{
  unpack v.0 Vec3 #0(x.1: float, y.2: float, z.3: float)
  return x.1
}}
"
    );
    assert_eq!(check_scopes(&param), Ok(()));
    let error = Err(
        "`v.0` is Vec3{float, float, float}, but no struct remains after flatten in `f`"
            .to_string(),
    );
    assert_eq!(check_flattened(&param), error);
    assert_eq!(check(&param), error);
    let ret = format!(
        "{VEC3}fn f(x.0: float) -> Vec3{{float, float, float}} {{
  let v.1: Vec3{{float, float, float}} = con Vec3 #0(x.0, x.0, x.0)
  return v.1
}}
"
    );
    assert_eq!(check_scopes(&ret), Ok(()));
    assert_eq!(
        check_flattened(&ret),
        Err(
            "the function returns Vec3{float, float, float}, but no struct remains after flatten in `f`"
                .to_string()
        )
    );
}

/// 複数の値の `return` は、関数の結果の並びと長さが同じである。複数の値の命令は開いた後の段だけに現れる
/// (docs/spec/lir.md の「verifier」)。
#[test]
fn a_return_list_of_the_wrong_length_is_rejected() {
    let text = |returned: &str| {
        format!(
            "fn g(x.0: float) -> (float, float) {{
  return (x.0, x.0)
}}
fn f(x.0: float) -> (float, float) {{
  let (a.1: float, b.2: float) = call g(x.0)
  let c.3: float = extern \"Prelude.Num Float.+\"(a.1, b.2)
  return {returned}
}}
"
        )
    };
    assert_eq!(check_flattened(&text("(c.3, a.1)")), Ok(()));
    assert_eq!(
        check_flattened(&text("(c.3, a.1, b.2)")),
        Err("a return gives 3 values, but the function returns (float, float) in `f`".to_string())
    );
    assert_eq!(
        check_flattened(&text("c.3")),
        Err("a return gives 1 values, but the function returns (float, float) in `f`".to_string())
    );
    // 開くパスより前の段は、複数の値を返す関数を拒む
    assert_eq!(
        check_scopes(&text("(c.3, a.1)")),
        Err("the function returns (float, float) before flatten in `g`".to_string())
    );
}

/// 構造体の `unbox` の箱を手放す `release` だけが `unboxed` の印を持ち、その箱の `release` はいつも印を持つ。VM は
/// 印の付いた `release` を、ダミーを受けても何もしない別の命令にする
/// (docs/spec/lir.md の「ダミーの値」)。
#[test]
fn only_the_box_of_a_struct_unbox_is_released_unboxed() {
    let text = |unbox: &str, release: &str| {
        format!(
            "layout P {{ P(obj, obj) }}
fn f(w.0: obj) -> obj {{
  {unbox}
  {release} w.0 P #0(a.1, _)
  return a.1
}}
"
        )
    };
    let unbox = "let (a.1: obj, _) = unbox w.0 P";
    let unpack = "unpack w.0 P #0(a.1: obj, b.2: obj)";
    assert_eq!(check(&text(unbox, "release unboxed")), Ok(()));
    assert_eq!(
        check(&text(unbox, "release")),
        Err(
            "`w.0` is unboxed as a struct, but its release is not marked unboxed in `f`"
                .to_string()
        )
    );
    assert_eq!(
        check(&text(unpack, "release unboxed")),
        Err("`w.0` is released as unboxed, but it is not unboxed as a struct in `f`".to_string())
    );
}
