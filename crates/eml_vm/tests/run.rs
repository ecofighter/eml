//! 分岐のない LIR の実行: 定数、extern、直接の呼び出しと戻り、末尾呼び出し、誤りと位置、終わりの処理
//! (docs/spec/vm.md の「呼び出しと戻り」と「誤り、止めること、終わり」)。

use eml_rt::{ArithFault, Fault};
use eml_vm::{SourceLocation, VmError, VmFault};

use crate::common::{run_lir, run_unverified};

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
