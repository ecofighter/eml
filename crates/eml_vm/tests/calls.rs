//! LIR のテキストで、クロージャの eval/apply を確かめる (docs/spec/core-ir.md)。handler をまたぐ再開は evidence の
//! 命令が要るので、`evidence.rs` で確かめる。関数の値として使う関数は一様なので、`Int` は `box` して渡し、結果は `unbox` して受ける。

use std::sync::Arc;

use eml_rt::{Control, Limits, Stop};
use eml_vm::{VmError, VmFault};

use crate::common::{parse, run_lir, run_unverified};

/// `first` は2つの引数のうち最初のものを返す。
const FIRST: &str = "\
fn first(a.0: tobj, b.1: tobj) -> tobj {
  decref b.1
  return a.0
}
";

/// `make` は `first` に1つめの引数だけを渡したクロージャを返す。
const MAKE: &str = "\
fn make(x.0: tobj) -> tobj {
  let c.1: tobj = closure first(x.0)
  return c.1
}
";

#[test]
fn a_partial_application_waits_for_the_rest_of_the_arguments() {
    let text = format!(
        "\
fn main() -> unit {{
  let a.0: tobj = box 10
  let d.1: tobj = apply &first(a.0)
  let b.2: tobj = box 20
  let q.3: tobj = apply d.1(b.2)
  let r.4: int = unbox q.3
  decref q.3
  let s.5: obj = extern \"Prelude.Show Int.show\"(r.4)
  let t.6: unit = extern Prelude.println(s.5)
  return t.6
}}
{FIRST}"
    );
    assert_eq!(run_lir(&text), ("10\n".to_string(), Ok(())));
}

#[test]
fn a_returned_function_can_still_wait_for_more_arguments() {
    let text = "\
fn main() -> unit {
  let a.0: tobj = box 5
  let b.1: tobj = box 6
  let r.2: tobj = apply &make(a.0, b.1)
  let c.3: tobj = box 7
  let q.4: tobj = apply r.2(c.3)
  let s.5: int = unbox q.4
  decref q.4
  let t.6: obj = extern \"Prelude.Show Int.show\"(s.5)
  let u.7: unit = extern Prelude.println(t.6)
  return u.7
}
fn first3(a.0: tobj, b.1: tobj, c.2: tobj) -> tobj {
  decref b.1
  decref c.2
  return a.0
}
fn make(x.0: tobj) -> tobj {
  let c.1: tobj = closure first3(x.0)
  return c.1
}
";
    assert_eq!(run_lir(text), ("5\n".to_string(), Ok(())));
}

#[test]
fn extra_arguments_are_applied_to_the_returned_function() {
    let text = format!(
        "\
fn main() -> unit {{
  let a.0: tobj = box 5
  let b.1: tobj = box 6
  let q.2: tobj = apply &make(a.0, b.1)
  let r.3: int = unbox q.2
  decref q.2
  let s.4: obj = extern \"Prelude.Show Int.show\"(r.3)
  let t.5: unit = extern Prelude.println(s.4)
  return t.5
}}
{FIRST}{MAKE}"
    );
    assert_eq!(run_lir(&text), ("5\n".to_string(), Ok(())));
}

#[test]
fn a_shared_closure_keeps_its_captured_values() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"a\"
  let c.1: tobj = closure first(s.0)
  dup c.1
  let n.2: tobj = box 1
  let r.3: obj = apply c.1(n.2) save [c.1]
  let t.4: unit = extern Prelude.println(r.3)
  let m.5: tobj = box 2
  let r.6: obj = apply c.1(m.5)
  let t.7: unit = extern Prelude.println(r.6)
  return t.7
}
fn first(a.0: obj, b.1: tobj) -> obj {
  decref b.1
  return a.0
}
";
    assert_eq!(run_lir(text), ("a\na\n".to_string(), Ok(())));
}

#[test]
fn a_function_value_is_applied_like_a_closure_without_arguments() {
    // 変数に入った関数の値で、等しい、足りない、余るの3つの場合を確かめる (docs/spec/core-ir.md の eval/apply)
    let text = format!(
        "\
fn main() -> unit {{
  tail call each(&first, &make)
}}
fn each(f.0: tobj, m.1: tobj) -> unit {{
  let a.2: tobj = box 10
  let d.3: tobj = apply f.0(a.2) save [m.1]
  let b.4: tobj = box 20
  let q.5: tobj = apply d.3(b.4) save [m.1]
  let r.6: int = unbox q.5
  decref q.5
  let s.7: obj = extern \"Prelude.Show Int.show\"(r.6)
  let t.8: unit = extern Prelude.println(s.7)
  let c.9: tobj = box 5
  let e.10: tobj = box 6
  let p.11: tobj = apply m.1(c.9, e.10)
  let r.12: int = unbox p.11
  decref p.11
  let s.13: obj = extern \"Prelude.Show Int.show\"(r.12)
  let t.14: unit = extern Prelude.println(s.13)
  return t.14
}}
{FIRST}{MAKE}"
    );
    assert_eq!(run_lir(&text), ("10\n5\n".to_string(), Ok(())));
}

#[test]
fn a_function_value_needs_no_reference_counting() {
    let text = format!(
        "\
layout (,) {{ (,)(tobj, tobj) }}
fn main() -> unit {{
  tail call pair(&first)
}}
fn pair(f.0: tobj) -> unit {{
  dup f.0
  let d.1: obj = con (,) #0(f.0, f.0)
  decref d.1
  let s.2: obj = const \"ok\"
  let t.3: unit = extern Prelude.println(s.2)
  return t.3
}}
{FIRST}"
    );
    assert_eq!(run_lir(&text), ("ok\n".to_string(), Ok(())));
}

/// `make` はクロージャを返し、そのクロージャはまた関数を返す。末尾の位置で、引数が2段余る。
#[test]
fn over_application_in_tail_position_nests() {
    let text = "\
fn main() -> unit {
  let a.0: tobj = box 1
  let b.1: tobj = box 2
  let c.2: tobj = box 3
  let r.3: tobj = apply &make(a.0, b.1, c.2)
  let n.4: int = unbox r.3
  decref r.3
  let s.5: obj = extern \"Prelude.Show Int.show\"(n.4)
  let u.6: unit = extern Prelude.println(s.5)
  return u.6
}
fn make(x.0: tobj) -> tobj {
  let c.1: tobj = closure pick(x.0)
  return c.1
}
fn pick(x.0: tobj, y.1: tobj) -> tobj {
  decref y.1
  let c.2: tobj = closure keep(x.0)
  return c.2
}
fn keep(x.0: tobj, z.1: tobj) -> tobj {
  decref z.1
  return x.0
}
";
    assert_eq!(run_lir(text), ("1\n".to_string(), Ok(())));
}

/// 末尾の位置の `apply` で引数が足りなければ、作ったクロージャを返す。
#[test]
fn a_tail_apply_with_too_few_arguments_returns_a_closure() {
    let text = "\
fn main() -> unit {
  let a.0: tobj = box 5
  let f.1: tobj = call partial(a.0)
  let b.2: tobj = box 6
  let r.3: tobj = apply f.1(b.2)
  let n.4: int = unbox r.3
  decref r.3
  let s.5: obj = extern \"Prelude.Show Int.show\"(n.4)
  let u.6: unit = extern Prelude.println(s.5)
  return u.6
}
fn partial(x.0: tobj) -> tobj {
  tail apply &first(x.0)
}
fn first(a.0: tobj, b.1: tobj) -> tobj {
  decref b.1
  return a.0
}
";
    assert_eq!(run_lir(text), ("5\n".to_string(), Ok(())));
}

const COUNT: &str = "\
layout Bool { False, True }
fn main() -> unit {
  tail call count(0)
}
fn count(n.0: int) -> unit {
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 100000)
  switch d.1 Bool { #0 -> b1, #1 -> b2 }
b1:
  let m.2: int = extern \"Prelude.+\"(n.0, 1)
  tail call count(m.2)
b2:
  return ()
}
";

/// 自己末尾呼び出しは入口へのジャンプなので、深さは反復の数によらない。上限を枠いくつかの深さにしても止まらない。
#[test]
fn a_self_tail_loop_runs_in_constant_depth() {
    let control = Arc::new(Control::new(Limits {
        max_depth: 64,
        ..Limits::default()
    }));
    let (host, _) = eml_vm::capture_host(Default::default());
    let result = eml_vm::execute_with(&parse(COUNT), true, host, control);
    assert!(result.is_ok(), "{result:?}");
}

/// 自己末尾呼び出しのジャンプも安全点なので、割り込みで止まる (docs/spec/vm.md の「呼び出しと戻り」)。
#[test]
fn a_self_tail_loop_stops_on_an_interrupt() {
    let control = Arc::new(Control::new(Limits::default()));
    control.interrupt();
    let (host, _) = eml_vm::capture_host(Default::default());
    let result = eml_vm::execute_with(&parse(COUNT), true, host, control);
    assert!(matches!(
        result,
        Err(VmError::Fault {
            fault: VmFault::Stopped(Stop::Interrupted),
            ..
        })
    ));
}

#[test]
fn deep_recursion_hits_the_depth_limit() {
    let text = "\
layout Bool { False, True }
fn main() -> unit {
  let r.0: unit = call down(100000)
  return r.0
}
fn down(n.0: int) -> unit {
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 0)
  switch d.1 Bool { #0 -> b1, #1 -> b2 }
b1:
  let m.2: int = extern \"Prelude.-\"(n.0, 1)
  let r.3: unit = call down(m.2)
  return r.3
b2:
  return ()
}
";
    let control = Arc::new(Control::new(Limits {
        max_depth: 1000,
        ..Limits::default()
    }));
    let (host, _) = eml_vm::capture_host(Default::default());
    let result = eml_vm::execute_with(&parse(text), true, host, control);
    assert_eq!(
        result.unwrap_err().to_string(),
        "the call depth limit is exceeded in `down`"
    );
}

/// 走査する語が 255 個以上のクロージャは、ペイロードの先頭に数の語を持つ。`apply` は `scanned` で読むので、
/// 捕まえた値の順が保たれる。
#[test]
fn a_closure_with_many_captures_applies_in_order() {
    let n = 300;
    let params: Vec<String> = (0..n).map(|i| format!("c.{i}: tobj")).collect();
    let mut text = String::from("fn main() -> unit {\n");
    for i in 0..n {
        text += &format!("  let b.{i}: tobj = box {i}\n");
    }
    let captured: Vec<String> = (0..n).map(|i| format!("b.{i}")).collect();
    text += &format!(
        "  let f.{n}: tobj = closure last({})\n",
        captured.join(", ")
    );
    text += &format!("  let r.{}: tobj = apply f.{n}(())\n", n + 1);
    text += &format!("  let k.{}: int = unbox r.{}\n", n + 2, n + 1);
    text += &format!("  decref r.{}\n", n + 1);
    text += &format!(
        "  let s.{}: obj = extern \"Prelude.Show Int.show\"(k.{})\n",
        n + 3,
        n + 2
    );
    text += &format!(
        "  let u.{}: unit = extern Prelude.println(s.{})\n",
        n + 4,
        n + 3
    );
    text += &format!("  return u.{}\n}}\n", n + 4);
    text += &format!("fn last({}, u.{n}: unit) -> tobj {{\n", params.join(", "));
    for i in 0..n - 1 {
        text += &format!("  decref c.{i}\n");
    }
    text += &format!("  return c.{}\n}}\n", n - 1);
    assert_eq!(run_lir(&text), (format!("{}\n", n - 1), Ok(())));
}

/// `clause` の引数の数は、いつも入口の関数の引数の数と等しい。違う IR は verifier を通らないので、通さずに実行する。
#[test]
fn a_clause_with_the_wrong_number_of_arguments_is_an_internal_error() {
    let text = "\
effect E { op(unit) -> unit }
fn main() -> unit {
  let r.0: unit = clause E.op &entry(())
  return r.0
}
internal clause E.op fn entry(e.0: obj, u.1: unit) -> unit {
  return ()
}
";
    let (_, result) = run_unverified(text);
    assert_eq!(
        result.unwrap_err().to_string(),
        "internal error: a clause gets a different number of arguments than its function takes in `main`"
    );
}

/// `ApplyRest` はどの関数にも属さないので、そこで起きた誤りには最後に戻った関数の名前を付ける。`make` は関数でない値
/// (即値の 7。関数は2つしかないので、7 はどの関数の番号でもない) を返すので、残りの引数を適用できない。
/// 結果が関数でないのは型の誤りで verifier は見ないが、通るので検査はそのまま通す。
#[test]
fn an_error_in_apply_rest_names_the_function_that_returned_last() {
    let text = "\
fn main() -> unit {
  let a.0: tobj = box 7
  let b.1: tobj = box 2
  let r.2: tobj = apply &make(a.0, b.1)
  decref r.2
  return ()
}
fn make(x.0: tobj) -> tobj {
  return x.0
}
";
    let (_, result) = run_unverified(text);
    assert_eq!(
        result.unwrap_err().to_string(),
        "internal error: applying a value that is not a function in `make`"
    );
}

/// 走査する語を持たない物体 (文字列) を適用すると、検査モードは走査する語の数を先に見て断る。数を見ずに 0 番目を読むと、
/// ヒープの範囲外の誤りになってしまう。文字列は関数でないので、verifier が型の誤りは見ないが、VM 自身の見張りを確かめる
/// ため通さずに実行する。
#[test]
fn applying_a_string_is_not_a_function() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"hello\"
  let r.1: tobj = apply s.0(())
  return ()
}
";
    let (_, result) = run_unverified(text);
    assert_eq!(
        result.unwrap_err().to_string(),
        "internal error: applying a value that is not a function in `main`"
    );
}

/// 引数が余る末尾の `apply` は、呼ぶ側の枠より大きい枠に余りを預ける。`g` の枠は、余りの `2 + 3` 語より小さい。
#[test]
fn a_tail_apply_with_leftover_arguments_grows_a_small_frame() {
    let text = "\
fn main() -> unit {
  let a.0: tobj = box 5
  let r.1: tobj = call g(a.0)
  let n.2: int = unbox r.1
  decref r.1
  let s.3: obj = extern \"Prelude.Show Int.show\"(n.2)
  let u.4: unit = extern Prelude.println(s.3)
  return u.4
}
fn g(x.0: tobj) -> tobj {
  dup x.0
  dup x.0
  dup x.0
  tail apply &f3(x.0, x.0, x.0, x.0)
}
fn f3(x.0: tobj) -> tobj {
  let c.1: tobj = closure h(x.0)
  return c.1
}
fn h(c.0: tobj, y.1: tobj, z.2: tobj, w.3: tobj) -> tobj {
  decref y.1
  decref z.2
  decref w.3
  return c.0
}
";
    assert_eq!(run_lir(text), ("5\n".to_string(), Ok(())));
}
