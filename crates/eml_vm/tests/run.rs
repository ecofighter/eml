//! LIR の実行: 定数、extern、直接の呼び出しと戻り (入口でないブロックからの戻りを含む)、末尾呼び出し、誤りと位置、
//! 終わりの処理 (docs/spec/vm.md の「呼び出しと戻り」と「誤り、止めること、終わり」)。

use eml_rt::{ArithFault, Fault, HeapError};
use eml_test_support::execute_vm;
use eml_vm::{SourceLocation, VmError, VmFault};

use crate::common::{parse, run_lir, run_unverified};

#[test]
fn hello_prints_a_line() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"hello\"
  let u.1: unit = extern Prelude.println(s.0)
  return u.1
}
";
    assert_eq!(run_lir(text), ("hello\n".to_string(), Ok(())));
}

#[test]
fn a_call_returns_its_result_to_the_caller() {
    let text = "\
fn main() -> unit {
  let n.0: int = call add(40, 2)
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
fn add(a.0: int, b.1: int) -> int {
  let c.2: int = extern \"Prelude.+\"(a.0, b.1)
  return c.2
}
";
    assert_eq!(run_lir(text), ("42\n".to_string(), Ok(())));
}

#[test]
fn an_int_wider_than_the_immediate_is_a_constant() {
    let text = "\
fn main() -> unit {
  let n.0: int = extern \"Prelude.+\"(4611686018427387904, 1)
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
";
    assert_eq!(run_lir(text), ("4611686018427387905\n".to_string(), Ok(())));
}

#[test]
fn a_tail_call_reuses_the_frame_header() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"tail\"
  tail call say(s.0)
}
fn say(s.0: obj) -> unit {
  let u.1: unit = extern Prelude.println(s.0)
  return u.1
}
";
    assert_eq!(run_lir(text), ("tail\n".to_string(), Ok(())));
}

#[test]
fn a_division_by_zero_carries_the_position_of_the_extern() {
    let text = "\
fn main() -> unit {
  let n.0: int = extern \"Prelude./\"(1, 0) @\"main.em\":2:3
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
";
    let (_, result) = run_lir(text);
    let error = result.unwrap_err();
    assert_eq!(
        error,
        VmError::Fault {
            fault: VmFault::Extern(Fault::Arith(ArithFault::DivisionByZero)),
            function: "main".to_string(),
            at: Some(SourceLocation {
                path: "main.em".to_string(),
                line: 2,
                column: 3,
            }),
        }
    );
    assert_eq!(error.to_string(), "division by zero\n  at main.em:2:3");
}

#[test]
fn output_before_an_error_is_flushed() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"before\"
  let u.1: unit = extern Prelude.println(s.0)
  let n.2: int = extern \"Prelude.%\"(1, 0)
  let t.3: obj = extern \"Prelude.Show Int.show\"(n.2)
  let v.4: unit = extern Prelude.println(t.3)
  return v.4
}
";
    let (out, result) = run_lir(text);
    assert_eq!(out, "before\n");
    assert_eq!(
        result.unwrap_err().to_string(),
        "division by zero in `main`"
    );
}

#[test]
fn an_entry_that_returns_an_object_does_not_leak() {
    let text = "\
fn main() -> obj {
  let s.0: obj = extern \"Prelude.Show Int.show\"(7)
  return s.0
}
";
    assert_eq!(run_lir(text), (String::new(), Ok(())));
}

/// verifier は手放さない値を拒むので、通さずに実行して検査モードのリークの検査を確かめる。
#[test]
fn a_forgotten_object_is_a_leak() {
    let text = "\
fn main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(7)
  return ()
}
";
    let (_, result) = run_unverified(text);
    let error = result.unwrap_err();
    assert_eq!(error, VmError::Leak(vec![("String".to_string(), 1)]));
    assert_eq!(
        error.to_string(),
        "memory leak: objects were not freed: 1 String"
    );
    // 速いモードはリークを確かめないので、同じ IR が成功で終わる
    let (_, fast) = execute_vm(&parse(text), false);
    assert_eq!(fast.map(|_| ()), Ok(()));
}

/// コード生成は `*ext as u32` を行の番号に使い、VM は `Extern::ALL[row]` で引くので、2つの順が同じでなければならない。
#[test]
fn extern_all_follows_the_declaration_order() {
    assert!(
        eml_extern::Extern::ALL
            .iter()
            .enumerate()
            .all(|(i, &e)| e as usize == i)
    );
}

#[test]
fn an_unused_call_result_does_not_overwrite_a_live_value() {
    let text = "\
fn main() -> unit {
  let k.0: int = extern \"Prelude.+\"(1, 2)
  let r.1: int = call one() save [k.0]
  let s.2: obj = extern \"Prelude.Show Int.show\"(k.0)
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
fn one() -> int {
  return 1
}
";
    assert_eq!(run_lir(text), ("3\n".to_string(), Ok(())));
}

#[test]
fn swapped_block_arguments_arrive_swapped() {
    let text = "\
fn main() -> unit {
  let x.0: int = extern \"Prelude.+\"(1, 0)
  let y.1: int = extern \"Prelude.+\"(2, 0)
  jump b1(y.1, x.0)
b1(p.2: int, q.3: int):
  let d.4: int = extern \"Prelude.-\"(p.2, q.3)
  let s.5: obj = extern \"Prelude.Show Int.show\"(d.4)
  let u.6: unit = extern Prelude.println(s.5)
  return u.6
}
";
    assert_eq!(run_lir(text), ("1\n".to_string(), Ok(())));
}

#[test]
fn a_call_resumes_at_the_next_statement_of_its_block() {
    // 入口でないブロックの2つの呼び出しから、それぞれ次の文に戻り、呼ぶ側のスロットに残る `n.0` を合流のブロック
    // まで持ち越す (docs/spec/vm.md の「呼び出しと戻り」)
    let text = "\
fn main() -> unit {
  let r.0: int = call pick(1)
  let s.1: obj = extern \"Prelude.Show Int.show\"(r.0)
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
    assert_eq!(run_lir(text), ("5\n".to_string(), Ok(())));
}

#[test]
fn a_fault_outside_an_extern_has_no_position() {
    // 位置を付けるのは extern の呼び出しが起こした誤りだけである。同じ関数に位置付きの extern があっても、`decref` の
    // 誤りには付けない
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn main() -> unit {
  let n.0: int = extern Prelude.+(1, 2) @\"main.em\":1:1
  let d.1: obj = con (,) #0(n.0, n.0)
  decref d.1
  decref d.1
  return ()
}
";
    let (_, result) = run_unverified(text);
    assert_eq!(
        result,
        Err(VmError::Fault {
            fault: VmFault::Extern(Fault::Heap(HeapError::UseAfterFree)),
            function: "main".to_string(),
            at: None,
        })
    );
}

#[test]
fn a_heap_fault_inside_an_extern_with_a_position_carries_it() {
    // 位置は extern の呼び出しが起こした誤りすべてに付き、ヒープの誤りも含む (docs/spec/vm.md の「誤り、止めること、
    // 終わり」)
    let text = "\
fn main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(1)
  decref s.0
  let t.1: unit = extern Prelude.println(s.0) @\"main.em\":2:3
  return t.1
}
";
    let (_, result) = run_unverified(text);
    let error = result.unwrap_err();
    assert_eq!(
        error,
        VmError::Fault {
            fault: VmFault::Extern(Fault::Heap(HeapError::UseAfterFree)),
            function: "main".to_string(),
            at: Some(SourceLocation {
                path: "main.em".to_string(),
                line: 2,
                column: 3,
            }),
        }
    );
    assert_eq!(error.to_string(), "use of a freed object\n  at main.em:2:3");
}

#[test]
fn a_dup_of_a_file_is_a_runtime_error() {
    // verifier は `lin` の変数の `dup` を V1 で静的に拒む。印を誤った IR でも、検査モードのヒープが実行時に止める
    // ことを、verifier を通さずに確かめる。速いモードはこれを見つけない (docs/spec/object-model.md の「`lin` ビット」と
    // 「受け入れる制約」)
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let text = format!(
        "\
fn main() -> unit {{
  let p.0: obj = const \"{path}\"
  let u.1: unit = call f(p.0)
  return u.1
}}
fn f(p.0: obj) -> unit {{
  let h.1: obj lin = extern Std.Fs.open(p.0)
  dup h.1
  let u.2: unit = extern Std.Fs.close(h.1)
  let u.3: unit = extern Std.Fs.close(h.1)
  return u.3
}}
"
    );
    assert_eq!(
        eml_lir::verify(&parse(&text)).map_err(|error| error.to_string()),
        Err("`h.1` (lin) is duplicated, but only unr can be in `f`".to_string())
    );
    let (_, result) = run_unverified(&text);
    let error = result.unwrap_err();
    assert_eq!(
        error,
        VmError::Fault {
            fault: VmFault::Extern(Fault::Heap(HeapError::LinDup)),
            function: "f".to_string(),
            at: None,
        }
    );
    assert_eq!(error.to_string(), "a Lin object is shared in `f`");
}

/// 種類が2つ以上のリークは、種類ごとの数を `, ` でつないで表示する。
#[test]
fn leaks_are_displayed_with_the_object_counts() {
    let leak = VmError::Leak(vec![("Closure".to_string(), 2), ("String".to_string(), 1)]);
    assert_eq!(
        leak.to_string(),
        "memory leak: objects were not freed: 2 Closure, 1 String"
    );
}
