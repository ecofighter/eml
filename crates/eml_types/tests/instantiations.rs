//! 参照ごとの具体化の表 (docs/implementation/architecture.md の「`eml_types` の内部」)。記録する参照、記録しない参照、
//! 型引数の順を確かめる。

use eml_hir::{ExprKind, Res, ValueItem};
use eml_test_support::{Checked, check, check_files, short_text};
use eml_types::TypeKind;

/// モジュール `module` の関数 `name` の本体の記録を、式の位置の順に1行ずつ `<行:列> <宣言の名前> [<型引数>]` の形で
/// 出す。位置はその関数のモジュールのファイルで数える。誤りのないプログラムだけを受け取る。
fn table(checked: &Checked, module: &str, name: &str) -> String {
    assert!(
        checked.diagnostics.is_empty(),
        "{}",
        short_text(checked.files(), &checked.diagnostics)
    );
    records(checked, module, name)
}

/// `table` と同じ形で出す。誤りのあるプログラムでも記録を見るために使う。
fn records(checked: &Checked, module: &str, name: &str) -> String {
    let program = &checked.program;
    let (id, _) = program
        .functions()
        .find(|(id, function)| program.modules[id.module].name == module && function.name == name)
        .unwrap();
    let body = program.body(id).unwrap();
    let file = program.file(id.module);
    let mut rows: Vec<_> = checked.typed.bodies[id]
        .instantiations
        .iter()
        .map(|(expr, instantiation)| {
            let start = body.exprs[expr].range.start();
            let decl = program.value_name(instantiation.decl);
            let args: Vec<String> = instantiation
                .args
                .iter()
                .map(|&arg| checked.typed.types.display(arg, &program.names).to_string())
                .collect();
            let at = checked.files().line_col(file, start);
            (start, format!("{at} {decl} [{}]\n", args.join(", ")))
        })
        .collect();
    rows.sort_by_key(|&(start, _)| start);
    rows.into_iter().map(|(_, row)| row).collect()
}

/// 入口のモジュールの関数 `name` の記録。
fn entry_table(text: &str, name: &str) -> String {
    table(&check(text), "Main", name)
}

const ID: &str = "id : a -> a\nid x = x\n\n";

#[test]
fn each_reference_to_a_polymorphic_function_is_recorded_on_its_own() {
    let text = format!("{ID}pair : Unit -> (Int, String)\npair () = (id 1, id \"a\")");
    insta::assert_snapshot!(entry_table(&text, "pair"), @"
    5:12 id [Int]
    5:18 id [String]
    ");
}

#[test]
fn a_user_constructor_is_recorded() {
    let text = "data Box a = | Box a\n\nboxed : Unit -> Box Int\nboxed () = Box 1";
    insta::assert_snapshot!(entry_table(text, "boxed"), @"4:12 Box [Int]");
}

#[test]
fn the_type_arguments_of_an_operation_start_with_those_of_its_effect() {
    let text = "effect State s where\n  swap : a -> s -> (a, s)\n\nswapped : Unit -> <State String> (Int, String)\nswapped () = swap 1 \"x\"";
    insta::assert_snapshot!(entry_table(text, "swapped"), @"5:14 swap [String, Int]");
}

#[test]
fn the_type_arguments_of_a_constructor_follow_the_head_of_its_data() {
    let text = "data P a b = | Q b a\n\nbuilt : Unit -> P Int String\nbuilt () = Q \"x\" 1";
    insta::assert_snapshot!(entry_table(text, "built"), @"4:12 Q [Int, String]");
}

#[test]
fn the_type_arguments_of_a_function_follow_their_first_appearance() {
    // `(<<) : (b -> <e> c) -> (a -> <e> b) -> a -> <e> c` の型引数は、名前の順ではなく `[b, c, a]` である
    let text =
        "composed : (Int -> String) -> (Bool -> Int) -> Bool -> String\ncomposed f g = f << g";
    insta::assert_snapshot!(entry_table(text, "composed"), @"2:18 << [Int, String, Bool]");
}

#[test]
fn a_type_argument_decided_by_a_later_statement_is_recorded() {
    // `x` の型は、後の文の `f 1` で `Int` に決まる
    let text = format!("{ID}later : Unit -> Int\nlater () =\n  let f = fn x -> id x\n  f 1");
    insta::assert_snapshot!(entry_table(&text, "later"), @"6:19 id [Int]");
}

#[test]
fn references_in_a_polymorphic_body_have_its_rigid_variables() {
    let text = format!("{ID}twice : a -> a\ntwice x = id (id x)");
    insta::assert_snapshot!(entry_table(&text, "twice"), @"
    5:11 id [a]
    5:15 id [a]
    ");
}

#[test]
fn an_undecided_type_argument_is_flexible() {
    let text =
        "data Option a = | None | Some a\n\nunused : Unit -> Int\nunused () =\n  let _ = None\n  0";
    insta::assert_snapshot!(entry_table(text, "unused"), @"5:11 None [_]");
}

#[test]
fn a_clause_variable_is_shown_like_the_function_variable_of_the_same_name() {
    // 節の `x` の型は操作ごとの型変数 `a` で、関数の `a` と別の型 (`OpVar`) だが、同じ名前で表示する
    // (docs/implementation/architecture.md の「`eml_types` の内部」)
    let text = format!(
        "{ID}effect Pick where\n  pick : a -> a\n\nrun : a -> a\nrun v =\n  handle id v with\n    | pick x k -> k (id x)"
    );
    insta::assert_snapshot!(entry_table(&text, "run"), @"
    9:10 id [a]
    10:22 id [a]
    ");
}

#[test]
fn a_clause_variable_is_a_different_type_from_the_function_variable_of_the_same_name() {
    // 単相化は関数の型変数にだけ代入するので、節の操作ごとの型変数は別の種類で書き出す
    // (docs/implementation/architecture.md の「`eml_types` の内部」)
    let text = format!(
        "{ID}effect Pick where\n  pick : a -> a\n\nrun : a -> a\nrun v =\n  handle id v with\n    | pick x k -> k (id x)"
    );
    let checked = check(&text);
    let program = &checked.program;
    let (id, _) = program
        .functions()
        .find(|(_, function)| function.name == "run")
        .unwrap();
    let args: Vec<TypeKind> = checked.typed.bodies[id]
        .instantiations
        .iter()
        .map(|(_, instantiation)| checked.typed.types.kind(instantiation.args[0]).clone())
        .collect();
    assert_eq!(
        args,
        [
            TypeKind::Rigid("a".to_string()),
            TypeKind::OpVar("a".to_string())
        ]
    );
}

#[test]
fn desugared_references_are_recorded_at_the_operator() {
    let text = "both : Int -> Bool -> Bool\nboth n b = n == 1 && b\n\nnegated : Int -> Int\nnegated n = - n\n\nsection : Unit -> Int -> Bool\nsection () = (== 1)";
    let checked = check(text);
    insta::assert_snapshot!(table(&checked, "Main", "both"), @"
    2:14 == [Int]
    2:19 False []
    ");
    insta::assert_snapshot!(table(&checked, "Main", "negated"), @"5:13 negate []");
    insta::assert_snapshot!(table(&checked, "Main", "section"), @"8:15 == [Int]");
}

#[test]
fn an_item_without_type_variables_has_no_type_arguments() {
    let text = "negation : Unit -> Bool\nnegation () = not True";
    insta::assert_snapshot!(entry_table(text, "negation"), @"
    2:15 not []
    2:19 True []
    ");
}

#[test]
fn references_in_another_module_are_recorded_in_its_body() {
    let m = "pub data Box a = | Box a\n\npub wrap : a -> Box a\nwrap x = Box x";
    let entry = "import M\n\nwrapped : Unit -> M.Box Int\nwrapped () = M.wrap 1";
    let checked = check_files(entry, &[("M.em", m)]);
    insta::assert_snapshot!(table(&checked, "M", "wrap"), @"4:10 Box [a]");
    insta::assert_snapshot!(table(&checked, "Main", "wrapped"), @"4:14 wrap [Int]");
}

#[test]
fn locals_and_constructor_patterns_are_not_recorded() {
    let text = "data Option a = | None | Some a\n\nget : Option Int -> Int\nget o = match o with\n  | Some n -> n\n  | None -> 0";
    insta::assert_snapshot!(entry_table(text, "get"), @"");
}

#[test]
fn every_reference_to_an_item_with_a_signature_is_recorded() {
    let text = "data Pair a b = | Pair a b\n\neffect Ask where\n  ask : Unit -> Int\n\nmixed : Int -> Bool -> <Ask> Pair Int Bool\nmixed n b =\n  let m = - n + ask ()\n  let p = (> 0)\n  let q = (n -)\n  Pair (m * 2) (b && p m || q 1 == 0)";
    let checked = check(text);
    assert!(
        checked.diagnostics.is_empty(),
        "{}",
        short_text(checked.files(), &checked.diagnostics)
    );
    let program = &checked.program;
    let mut recorded = 0;
    for (id, _) in program.functions() {
        let (Some(body), Some(types)) = (program.body(id), checked.typed.bodies.get(id)) else {
            continue;
        };
        for (expr, instantiation) in types.instantiations.iter() {
            let decl = match body.exprs[expr].kind {
                ExprKind::Path(Res::Item(item)) => item,
                ref kind => panic!("a record is keyed by a reference to an item, not {kind:?}"),
            };
            assert_eq!(instantiation.decl, decl);
            let rigids = match decl {
                ValueItem::Function(id) => program[id]
                    .signature
                    .as_ref()
                    .unwrap()
                    .generics
                    .type_vars
                    .len(),
                ValueItem::Operation(id) => program[id].signature.generics.type_vars.len(),
                ValueItem::Constructor(id) => program[program[id].ty].generics.type_vars.len(),
                ValueItem::Method(id) => program[id].signature.generics.type_vars.len(),
            };
            assert_eq!(instantiation.args.len(), rigids, "{decl:?}");
        }
        for (expr, data) in body.exprs.iter() {
            let decl = match data.kind {
                ExprKind::Path(Res::Item(item)) => item,
                _ => continue,
            };
            if checked.typed.decls.contains_key(&decl) {
                assert!(
                    types.instantiations.get(expr).is_some(),
                    "{decl:?} at {:?}",
                    data.range
                );
                if id.module == program.entry {
                    recorded += 1;
                }
            }
        }
    }
    // `-` (`negate`)、`+`、`ask`、`>`、`-`、`Pair`、`*`、`&&` の `False`、`||` の `True`、`==`
    assert_eq!(recorded, 10);
}

#[test]
fn a_reference_used_as_a_value_is_recorded() {
    let text = format!(
        "{ID}apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nused : Unit -> Int\nused () = apply id 1"
    );
    insta::assert_snapshot!(entry_table(&text, "used"), @"
    8:11 apply [Int, Int]
    8:17 id [Int]
    ");
}

#[test]
fn a_reference_without_a_signature_is_not_recorded() {
    let text = "f x = x\n\ng : Int -> Int\ng n = f n";
    let checked = check(text);
    insta::assert_snapshot!(short_text(checked.files(), &checked.diagnostics), @"E1004 1:1 `f` has no type signature");
    insta::assert_snapshot!(records(&checked, "Main", "g"), @"");
}

#[test]
fn a_comparison_reported_as_not_comparable_is_still_recorded() {
    let text = "poly : a -> a -> Bool\npoly x y = x == y";
    let checked = check(text);
    insta::assert_snapshot!(short_text(checked.files(), &checked.diagnostics), @"E2006 2:14 no instance of `Eq` for `a`");
    insta::assert_snapshot!(records(&checked, "Main", "poly"), @"2:14 == [a]");
}
