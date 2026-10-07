//! ソースから実行して確かめるテストは UI テスト (`tests/ui/run/`) に置く。ここには、Core IR のテキストや生成した
//! ソースが要るものだけを置く (docs/implementation/testing.md)。

use std::fmt::Write;

use eml_core_ir::Program;
use eml_interp::RuntimeError;
use eml_test_support::{execute, run};

use crate::common::run_core;

fn main_with(body: &str) -> String {
    format!("main : Unit -> <IO> Unit\nmain () =\n{body}")
}

/// Perceus の挿入を経ない手書きの Core IR で、`debug_heap` がリークを見つけることを確かめる。
fn leaking_program() -> Program {
    eml_core_ir::parse(
        r#"
fn main() {
  let s1^ = const "leaked"
  return ()
}
"#,
    )
    .unwrap_or_else(|error| panic!("{error}"))
}

/// `mask` 付きの呼び出しの中の操作は、呼び出しより外側の同じエフェクトの handler を、`mask` に並ぶ数だけ飛ばす。
/// 呼び出しの中で設けた handler は飛ばさない (docs/spec/core-ir.md)。`ir/mask.core` は `Ask` の handler を2つ積む。
/// 外側の handler は `outer`、内側の handler は `inner` を返す。内側の handler の下で、次の順に `ask` する。
///
/// 1. 非末尾の `mask` 付きの `call` で `ask` する (`outer`)
/// 2. `mask` 付きの `apply` の中で3つめの handler を設けて `ask` する。その handler が `new` を返す
/// 3. `Yield` の節が `mask` 付きの `resume` で本体を再開し、本体が `ask` する。この `ask` は `Yield` の handler を
///    越えて `Ask` の handler を探す (`outer`)。`resume` が今の継続を読む前に `Mask` フレームを積むので、`Mask`
///    フレームは再開した handler の下に入る。`Mask` フレームを積まないか、継続を読んだ後に積むと、ここは `inner` になり、
///    出力は `outer`、`new`、`inner`、`inner`、`outer` になる
/// 4. 3の `resume` から戻った節で `ask` する。`Mask` フレームはもう外れているので `inner` になる
/// 5. 末尾の `mask` 付きの `apply` で `ask` する (`outer`)
///
/// Core IR のテキストにはコメントを書けないので、IR の説明はここに書く。
#[test]
fn a_mask_skips_outer_handlers_only() {
    let (stdout, result) = run_core(include_str!("ir/mask.core"), true);
    assert_eq!(result, Ok(()));
    insta::assert_snapshot!(stdout, @"
    outer
    new
    outer
    inner
    outer
    ");
}

#[test]
fn debug_heap_reports_leaks() {
    assert_eq!(
        execute(leaking_program(), true).1,
        Err(RuntimeError::Leak(vec![("String".to_string(), 1)]))
    );
    assert_eq!(execute(leaking_program(), false).1, Ok(()));
}

#[test]
fn long_statement_sequence_does_not_overflow_the_stack() {
    // 逐次の文は入れ子ではないので、後段は長い `Let` の連鎖を再帰せずに処理しなければならない
    let mut body = "  let s = \"x\"\n".repeat(5000);
    body.push_str("  println s");
    assert_eq!(run(&main_with(&body)), ("x\n".to_string(), Ok(())));
}

#[test]
fn long_sequence_of_if_statements_does_not_overflow_the_stack() {
    // 文の `if` は join point になり、続きの文はその本体に入れ子になる。後段は、この入れ子を再帰せずに処理しなければならない
    let mut body = "  if True then println \"x\"\n".repeat(5000);
    body.push_str("  println \"done\"");
    let expected = format!("{}done\n", "x\n".repeat(5000));
    assert_eq!(run(&main_with(&body)), (expected, Ok(())));
}

#[test]
fn a_match_with_a_thousand_literals_runs_in_a_debug_build() {
    // 平らな `Switch` は、リテラルの数だけ入れ子を深くしない (docs/spec/core-ir.md)
    let mut text = String::from("pick : Int -> Int\npick n =\n  match n with\n");
    for i in 0..1000 {
        writeln!(text, "    | {i} -> {}", i + 1).unwrap();
    }
    text.push_str(
        "    | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick 999))\n",
    );
    let (out, result) = eml_test_support::run(&text);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "1000\n");
}
