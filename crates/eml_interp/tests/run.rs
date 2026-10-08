//! ソースから実行して確かめるテストは UI テスト (`tests/ui/run/`) に置く。ここには、Core IR のテキストや生成した
//! ソースが要るものだけを置く (docs/implementation/testing.md)。Core IR のテキストでは、制御の移り方 (ブロック、文、
//! `jump`、呼び出しからの再開)、handler、実行時エラーの位置を確かめる (docs/spec/core-ir.md)。

use std::fmt::Write;

use eml_interp::{Fault, RuntimeError, SourceLocation};
use eml_test_support::{execute, run};

use crate::common::{parse, run_core, run_core_unverified};

/// `mask` 付きの呼び出しの中の操作は、呼び出しより外側の同じエフェクトの handler を、`mask` に並ぶ数だけ飛ばす。
/// 呼び出しの中で設けた handler は飛ばさない (docs/spec/core-ir.md)。`ir/mask.core` は `Ask` の handler を2つ
/// 積む。外側の handler は `outer`、内側の handler は `inner` を返す。内側の handler の下で、次の順に `ask` する。
///
/// 1. 末尾でない `mask` 付きの `call` で `ask` する (`outer`)
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
    let (stdout, result) = run_core(include_str!("ir/mask.core"));
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
    // Perceus を経ない手書きの IR で、捨てていない文字列を `debug_heap` が見つける
    let program = parse(
        "\
fn main() -> unit {
  let s.0: obj = const \"leaked\"
  return ()
}
",
    );
    assert_eq!(
        execute(program.clone(), true).1,
        Err(RuntimeError::Leak(vec![("String".to_string(), 1)]))
    );
    assert_eq!(execute(program, false).1, Ok(()));
}

#[test]
fn a_call_resumes_at_the_next_statement_of_its_block() {
    // 戻りのフレームは、呼び出しの文の (ブロック, 文) を再開の番地に持つ。入口でないブロックの2つの呼び出しから、それぞれ
    // 次の文に戻り、退避した `n.0` を合流のブロックまで持ち越す
    let text = "\
fn main() -> unit {
  let r.0: int = call pick(1)
  let s.1: obj = extern Prelude.show_int(r.0)
  let t.2: unit = extern Prelude.println(s.1)
  return t.2
}
fn pick(n.0: int) -> int {
  switch n.0 { 1 -> b1, _ -> b2 }
b1:
  let a.1: int = call double(n.0) save [n.0]
  let b.2: int = call double(a.1) save [n.0]
  jump b3(b.2)
b2:
  jump b3(0)
b3(m.3: int):
  let c.4: int = extern Prelude.+(m.3, n.0)
  return c.4
}
fn double(x.0: int) -> int {
  let y.1: int = extern Prelude.+(x.0, x.0)
  return y.1
}
";
    assert_eq!(run_core(text), ("5\n".to_string(), Ok(())));
}

#[test]
fn a_jump_reads_every_argument_before_writing_the_parameters() {
    // 引数の入れ替えは、順に代入すると両方が `y.1` の値になる。verifier は同じ変数の2回の定義を拒むので、検証しない
    let text = "\
fn main() -> unit {
  tail call swap(1, 2)
}
fn swap(x.0: int, y.1: int) -> unit {
  jump b1(y.1, x.0)
b1(x.0: int, y.1: int):
  let s.2: obj = extern Prelude.show_int(x.0)
  let t.3: unit = extern Prelude.println(s.2)
  let s.4: obj = extern Prelude.show_int(y.1)
  let t.5: unit = extern Prelude.println(s.4)
  return t.5
}
";
    assert_eq!(run_core_unverified(text), ("2\n1\n".to_string(), Ok(())));
}

/// 0 で割る extern の呼び出しを、入口でないブロックに置く。`{position}` は extern の後ろに付ける位置である。
fn divide_by_zero(position: &str) -> String {
    format!(
        "\
fn main() -> unit {{
  let r.0: int = call divide(0)
  return ()
}}
fn divide(d.0: int) -> int {{
  switch d.0 {{ 1 -> b1, _ -> b2 }}
b1:
  return 1
b2:
  let q.1: int = extern Prelude./(1, d.0){position}
  return q.1
}}
"
    )
}

#[test]
fn an_extern_with_a_position_puts_it_on_its_runtime_error() {
    let (_, result) = run_core(&divide_by_zero(" @\"src/main.em\":3:9"));
    let error = result.unwrap_err();
    assert_eq!(
        error,
        RuntimeError::Fault {
            fault: Fault::DivisionByZero,
            function: "divide".to_string(),
            at: Some(SourceLocation {
                path: "src/main.em".to_string(),
                line: 3,
                column: 9,
            }),
        }
    );
    assert_eq!(error.to_string(), "division by zero\n  at src/main.em:3:9");
}

#[test]
fn an_extern_without_a_position_names_the_function() {
    let (_, result) = run_core(&divide_by_zero(""));
    let error = result.unwrap_err();
    assert_eq!(
        error,
        RuntimeError::Fault {
            fault: Fault::DivisionByZero,
            function: "divide".to_string(),
            at: None,
        }
    );
    assert_eq!(error.to_string(), "division by zero in `divide`");
}

#[test]
fn a_fault_outside_an_extern_has_no_position() {
    // 位置を付けるのは extern の呼び出しが起こした誤りだけである。同じ関数に位置付きの extern があっても、`decref` の
    // 誤りには付けない
    let text = "\
fn main() -> unit {
  let n.0: int = extern Prelude.+(1, 2) @\"main.em\":1:1
  let d.1: obj = con #0(n.0, n.0)
  decref d.1
  decref d.1
  return ()
}
";
    let (_, result) = run_core_unverified(text);
    assert_eq!(
        result,
        Err(RuntimeError::Fault {
            fault: Fault::Heap(eml_runtime::HeapError::UseAfterFree),
            function: "main".to_string(),
            at: None,
        })
    );
}

fn main_with(body: &str) -> String {
    format!("main : Unit -> <IO> Unit\nmain () =\n{body}")
}

#[test]
fn long_statement_sequence_does_not_overflow_the_stack() {
    // 逐次の文は入れ子ではないので、後段は長い文の列を再帰せずに処理しなければならない
    let mut body = "  let s = \"x\"\n".repeat(5000);
    body.push_str("  println s");
    assert_eq!(run(&main_with(&body)), ("x\n".to_string(), Ok(())));
}

#[test]
fn long_sequence_of_if_statements_does_not_overflow_the_stack() {
    // 文の `if` はそれぞれ `switch` と合流するブロックになり、ブロックの列が文の数だけ長くなる。後段は、この列を再帰
    // せずに処理しなければならない。条件は引数にする。`if True` は translate がその場で枝を選び、`switch` を出さない
    // ためである
    const COUNT: usize = 5000;
    let mut text = String::from("step : Bool -> <IO> Unit\nstep b =\n");
    text.push_str(&"  if b then println \"x\"\n".repeat(COUNT));
    text.push_str("  println \"done\"\n\nmain : Unit -> <IO> Unit\nmain () = step True");
    let program = eml_test_support::core(&text);
    let step = program
        .functions
        .iter()
        .find(|function| function.name == "step")
        .expect("step");
    let switches = step
        .blocks
        .iter()
        .filter(|block| matches!(block.term, eml_core_ir::Term::Switch { .. }))
        .count();
    assert_eq!(switches, COUNT);
    let expected = format!("{}done\n", "x\n".repeat(COUNT));
    assert_eq!(execute(program, true), (expected, Ok(())));
}

#[test]
fn a_match_with_a_thousand_literals_runs_in_a_debug_build() {
    // 平らな `switch` は、リテラルの数だけ入れ子を深くしない (docs/spec/core-ir.md)
    let mut text = String::from("pick : Int -> Int\npick n =\n  match n with\n");
    for i in 0..1000 {
        writeln!(text, "    | {i} -> {}", i + 1).unwrap();
    }
    text.push_str(
        "    | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick 999))\n",
    );
    let (out, result) = run(&text);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "1000\n");
}

#[test]
fn a_match_with_a_thousand_shared_arms_runs_in_a_debug_build() {
    // `(_, i)` の枝には、`A` の行き先と `default` の2つの葉が向かう。合流するブロックは入れ子にならないので、枝の数
    // だけ後段のスタックを使わない。scrutinee は引数にする。リテラルのタプルを渡すと、translate がその場で枝を選ぶ
    // ためである
    let mut text = String::from(
        "data AB =\n  | A\n  | B\n\npick : AB -> Int -> Int\npick t n = match (t, n) with\n  | (A, 0) -> 0\n",
    );
    for i in 1..=1000 {
        writeln!(text, "  | (_, {i}) -> {i}").unwrap();
    }
    text.push_str(
        "  | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick B 999))\n",
    );
    let (out, result) = run(&text);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "999\n");
}
