//! v2 の Core IR のテキストで、制御の移り方 (ブロック、文、`jump`、呼び出しからの再開)、handler、実行時エラーの
//! 位置を確かめる (docs/spec/core-ir.md)。

use eml_interp::{Fault, RuntimeError, SourceLocation};

use crate::common::{execute_v2, parse_v2, run_v2, run_v2_unverified};

/// `mask` 付きの呼び出しの中の操作は、呼び出しより外側の同じエフェクトの handler を、`mask` に並ぶ数だけ飛ばす。
/// 呼び出しの中で設けた handler は飛ばさない (docs/spec/core-ir.md)。`ir/v2_mask.core` は `Ask` の handler を2つ
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
    let (stdout, result) = run_v2(include_str!("ir/v2_mask.core"));
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
    let program = parse_v2(
        "\
fn main() -> unit {
  let s.0: obj = const \"leaked\"
  return ()
}
",
    );
    assert_eq!(
        execute_v2(&program, true).1,
        Err(RuntimeError::Leak(vec![("String".to_string(), 1)]))
    );
    assert_eq!(execute_v2(&program, false).1, Ok(()));
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
    assert_eq!(run_v2(text), ("5\n".to_string(), Ok(())));
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
    assert_eq!(run_v2_unverified(text), ("2\n1\n".to_string(), Ok(())));
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
    let (_, result) = run_v2(&divide_by_zero(" @\"src/main.em\":3:9"));
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
    let (_, result) = run_v2(&divide_by_zero(""));
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
    let (_, result) = run_v2_unverified(text);
    assert_eq!(
        result,
        Err(RuntimeError::Fault {
            fault: Fault::Heap(eml_runtime::HeapError::UseAfterFree),
            function: "main".to_string(),
            at: None,
        })
    );
}
