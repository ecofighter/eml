//! 具体化した row が yield しうるかの分類 (docs/spec/types.md の
//! 「row の具体化の記録」)。参照の `rows`、関数の本体、ラムダ、呼び出しの矢印の分類を確かめる。

use eml_hir::FunctionId;
use eml_test_support::{Checked, check, short_text};
use eml_types::RowYield;

/// 入口のモジュールの関数 `name`。誤りのないプログラムだけを受け取る。
fn entry(checked: &Checked, name: &str) -> FunctionId {
    assert_eq!(short_text(checked.files(), &checked.diagnostics), "");
    let program = &checked.program;
    let (id, _) = program
        .functions()
        .find(|(id, function)| program.modules[id.module].name == "Main" && function.name == name)
        .unwrap();
    id
}

fn list(rows: &[RowYield]) -> String {
    let rows: Vec<String> = rows.iter().map(|row| format!("{row:?}")).collect();
    rows.join(", ")
}

/// 参照ごとの row の分類を、式の位置の順に `<行:列> <宣言の名前> [<分類>, …]` の形で出す。
fn rows(text: &str, name: &str) -> String {
    let checked = check(text);
    let id = entry(&checked, name);
    let program = &checked.program;
    let body = program.body(id).unwrap();
    let file = program.file(id.module);
    let mut lines: Vec<_> = checked.typed.bodies[id]
        .instantiations
        .iter()
        .map(|(expr, instantiation)| {
            let start = body.exprs[expr].range.start();
            let decl = program.value_name(instantiation.decl);
            let at = checked.files().line_col(file, start);
            (
                start,
                format!("{at} {decl} [{}]\n", list(&instantiation.rows)),
            )
        })
        .collect();
    lines.sort_by_key(|&(start, _)| start);
    lines.into_iter().map(|(_, line)| line).collect()
}

/// 関数の本体の分類を `body <分類>` の1行目に出し、続けてラムダを `<行:列> <分類>`、呼び出しの矢印を
/// `<行:列>#<矢印> <分類>` の形で式の位置の順に出す。
fn bodies(text: &str, name: &str) -> String {
    let checked = check(text);
    let id = entry(&checked, name);
    let program = &checked.program;
    let body = program.body(id).unwrap();
    let file = program.file(id.module);
    let types = &checked.typed.bodies[id];
    let at = |expr: eml_hir::ExprId| {
        let start = body.exprs[expr].range.start();
        (start, checked.files().line_col(file, start))
    };
    let lambdas = types.lambda_yields.iter().map(|(expr, row)| {
        let (start, at) = at(expr);
        ((start, None), format!("{at} {row:?}\n"))
    });
    let arrows = types.arrow_yields.iter().map(|(&(expr, arrow), row)| {
        let (start, at) = at(expr);
        ((start, Some(arrow)), format!("{at}#{arrow} {row:?}\n"))
    });
    let mut lines: Vec<_> = lambdas.chain(arrows).collect();
    // 二項演算の呼び出しは左の項と同じ位置から始まるので、同じ位置の行は中身の順に並べて出力を決める
    lines.sort();
    let mut out = format!("body {:?}\n", types.body_yield);
    out.extend(lines.into_iter().map(|(_, line)| line));
    out
}

#[test]
fn a_handleable_label_always_yields_and_io_never_does() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Unit -> <Ask, IO> Int\nf () = ask ()\n\ng : Unit -> <IO> Unit\ng () = println \"x\"";
    insta::assert_snapshot!(bodies(text, "f"), @"
    body Always
    5:8#0 Always
    ");
    insta::assert_snapshot!(bodies(text, "g"), @"
    body Never
    8:8#0 Never
    ");
}

#[test]
fn a_row_variable_of_the_definition_is_var() {
    let text = "twice : (Unit -> <e> Int) -> <e> Int\ntwice f = f () + f ()";
    insta::assert_snapshot!(bodies(text, "twice"), @"
    body Var(0)
    2:11#0 Never
    2:11#0 Var(0)
    2:11#1 Never
    2:18#0 Var(0)
    ");
}

#[test]
fn the_rows_of_a_reference_follow_the_signature_order() {
    let text = "twice : (Unit -> <e> Int) -> <e> Int\ntwice f = f () + f ()\n\nmain : Unit -> <IO> Unit\nmain () = println (show (twice (fn () -> 1)))";
    insta::assert_snapshot!(rows(text, "main"), @"
    5:11 println []
    5:20 show []
    5:26 twice [Never]
    ");
}

#[test]
fn a_reference_classifies_each_row_variable_against_the_enclosing_definition() {
    // `pick` の row 変数は e、f の順で、e には Ask が、f には `user` の row 変数 g が入る
    let text = "effect Ask where\n  ask : Unit -> Int\n\npick : (Unit -> <e> Int) -> (Unit -> <f> Int) -> <e> Int\npick a b = a ()\n\nuser : (Unit -> <g> Int) -> <Ask | g> Int\nuser c = pick (fn () -> ask ()) c";
    insta::assert_snapshot!(rows(text, "user"), @"
    8:10 pick [Always, Var(0)]
    8:25 ask []
    ");
}

#[test]
fn an_operation_row_variable_in_a_clause_always_yields() {
    let text = "effect Run where\n  run : (Unit -> <e> Int) -> Int\n\nbody : Unit -> <Run> Int\nbody () = run (fn () -> 1)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle body () with\n      | run f k ->\n        let g = fn () -> f () + 1\n        k 7\n  println (show r)";
    // 節の中のラムダ g の row は操作の row 変数で、main の row 変数ではない
    insta::assert_snapshot!(bodies(text, "main"), @"
    body Never
    10:12#0 Always
    12:17 Always
    12:26#0 Always
    12:26#0 Never
    12:26#1 Never
    13:9#0 Never
    14:3#0 Never
    14:12#0 Never
    ");
}

#[test]
fn an_unresolved_row_variable_never_yields() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let h = fn f -> f 1 + 1\n  println \"x\"";
    insta::assert_snapshot!(bodies(text, "main"), @"
    body Never
    3:11 Never
    3:19#0 Never
    3:19#0 Never
    3:19#1 Never
    4:3#0 Never
    ");
}

#[test]
fn the_first_arrow_of_the_declared_type_is_classified_without_parameters() {
    // 等式に引数がなければ本体は純粋な値だが、最初の矢印の row は宣言の型のまま分類する
    let text = "effect Ask where\n  ask : Unit -> Int\n\nact : Unit -> <Ask> Int\nact = fn () -> ask ()\n\ngo : Unit -> <e> Int\ngo = fn () -> 1\n\nn : Int\nn = 1";
    let checked = check(text);
    let first = |name: &str| {
        let types = &checked.typed.bodies[entry(&checked, name)];
        format!("{:?} {:?}", types.body_yield, types.first_arrow_yield)
    };
    assert_eq!(first("act"), "Never Always");
    assert_eq!(first("go"), "Never Var(0)");
    assert_eq!(first("n"), "Never Never");
}

#[test]
fn the_second_row_variable_and_a_swapped_reference_keep_the_signature_order() {
    // `second` の本体は2つ目の row 変数 f で動く。`user` は自分の row 変数 g、h を、`second` の e に h、f に g の
    // 順で渡す。row 変数の並びを取り違えると、本体は `Var(0)` に、参照は `[Var(0), Var(1)]` になる
    let text = "second : (Unit -> <e> Int) -> (Unit -> <f> Int) -> <f> Int\nsecond a b = b ()\n\nuser : (Unit -> <g> Int) -> (Unit -> <h> Int) -> <g> Int\nuser c d = second d c";
    insta::assert_snapshot!(bodies(text, "second"), @"
    body Var(1)
    2:14#0 Var(1)
    ");
    insta::assert_snapshot!(rows(text, "user"), @"5:12 second [Var(1), Var(0)]");
}
