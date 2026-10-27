//! 巻き戻しの点 (`eml_lir::unwind::points`)。入力は所有の段の verifier を通る、Perceus の後の LIR のテキストである
//! (docs/spec/vm.md の「巻き戻しの表」)。

use eml_lir::unwind::{UnwindPoint, may_abort, points};
use eml_lir::{BlockId, VarId, parse, verify};

/// `name` の関数の巻き戻しの点。入力が所有の段の verifier を通ることも確かめる。
fn points_of(text: &str, name: &str) -> Vec<UnwindPoint> {
    let program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = verify(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    let index = program
        .functions
        .iter()
        .position(|function| function.name == name)
        .unwrap_or_else(|| panic!("no `{name}`"));
    points(&program, |_| true)
        .swap_remove(index)
        .expect("every function is wanted")
        .unwrap_or_else(|error| panic!("{error}"))
}

fn point(block: u32, stmt: usize, owned: &[(u32, u32)], landing: Option<u32>) -> UnwindPoint {
    UnwindPoint {
        block: BlockId(block),
        stmt,
        owned: owned
            .iter()
            .map(|&(var, count)| (VarId(var), count))
            .collect(),
        tokens: Vec::new(),
        landing: landing.map(VarId),
    }
}

/// `name` の関数が中断しうるか。
fn aborts(text: &str, name: &str) -> bool {
    let program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    let index = program
        .functions
        .iter()
        .position(|function| function.name == name)
        .unwrap_or_else(|| panic!("no `{name}`"));
    may_abort(&program)[index]
}

#[test]
fn call_lists_the_owned_variables_live_after_it() {
    // 呼び出しの束縛 `r` は、呼び出しが戻らなければまだないので数えない
    let text = "\
fn g(x.0: obj) -> obj {
  let z.1: obj = extern Prelude.exit(1)
  decref x.0
  return z.1
}
fn f(a.0: obj, b.1: obj) -> obj {
  let r.2: obj = call g(a.0)
  decref r.2
  return b.1
}
";
    assert_eq!(points_of(text, "f"), [point(0, 0, &[(1, 1)], None)]);
    assert_eq!(points_of(text, "g"), [point(0, 0, &[(0, 1)], None)]);
}

#[test]
fn a_dupped_variable_is_listed_with_its_count() {
    let text = "\
fn g(x.0: obj) -> obj {
  let z.1: obj = extern Prelude.exit(1)
  decref x.0
  return z.1
}
fn f(a.0: obj, b.1: obj) -> obj {
  dup b.1
  dup b.1
  let r.2: obj = call g(a.0)
  decref r.2
  let s.3: obj = extern Prelude.++(b.1, b.1)
  decref s.3
  return b.1
}
";
    assert_eq!(points_of(text, "f"), [point(0, 2, &[(1, 3)], None)]);
}

#[test]
fn a_borrowed_field_crossing_a_call_is_not_listed() {
    // `s` は `p` から借りているので、巻き戻しで手放すのは持ち主の `p` だけである
    let text = "\
layout P { P(obj) }
fn g(x.0: obj) -> obj {
  let z.1: obj = extern Prelude.exit(1)
  decref x.0
  return z.1
}
fn f(p.0: obj, x.1: obj) -> obj {
  switch p.0 P { #0(s.2: obj) -> b1 }
b1:
  let r.3: obj = call g(x.1)
  decref r.3
  dup s.2
  decref p.0
  return s.2
}
";
    assert_eq!(points_of(text, "f"), [point(1, 0, &[(0, 1)], None)]);
}

#[test]
fn effectful_extern_and_yield_abort_are_points() {
    let text = "\
fn f(s.0: obj, t.1: obj, e.2: obj, c.3: tobj) -> obj {
  let u.5: unit = extern Prelude.println(s.0)
  let w.6: unit = prim yield_abort(e.2, c.3)
  return t.1
}
fn g(t.0: obj) -> obj {
  let z.1: obj = extern Prelude.exit(1)
  decref t.0
  return z.1
}
";
    assert_eq!(
        points_of(text, "f"),
        [
            point(0, 0, &[(1, 1), (2, 1), (3, 1)], None),
            point(0, 1, &[(1, 1)], None),
        ]
    );
    assert_eq!(points_of(text, "g"), [point(0, 0, &[(0, 1)], None)]);
}

#[test]
fn pure_extern_and_tail_call_are_not() {
    let text = "\
fn g(x.0: obj) -> obj {
  return x.0
}
fn f(n.0: int, s.1: obj) -> obj {
  let m.2: obj = extern \"Prelude.Show Int.show\"(n.0)
  decref m.2
  tail call g(s.1)
}
";
    assert_eq!(points_of(text, "f"), []);
}

/// `Panic` を起こす extern (`Panics`) も、誤りのとき `try_panic` まで巻き戻すので点である
/// (docs/spec/vm.md の「巻き戻しの表」)。
#[test]
fn panicking_extern_is_a_point() {
    let text = "\
fn f(n.0: int, s.1: obj) -> obj {
  let q.2: int = extern \"Prelude.Num Int.+\"(n.0, 2)
  let t.3: obj = extern Prelude.panic(s.1)
  return t.3
}
";
    assert_eq!(
        points_of(text, "f"),
        [point(0, 0, &[(1, 1)], None), point(0, 1, &[], None)]
    );
}

#[test]
fn landing_is_the_prompt_variable() {
    let text = "\
yields fn f(b.0: tobj, ev.1: obj) -> obj {
  let x.2: tobj = yields apply b.0(()) prompt ev.1
  decref x.2
  return ev.1
}
";
    assert_eq!(points_of(text, "f"), [point(0, 0, &[(1, 1)], Some(1))]);
}

#[test]
fn clause_call_is_a_point() {
    let text = "\
effect Ask { ask(unit) -> tobj }
fn f(c.0: tobj, e.1: obj, k.2: obj) -> obj {
  let x.3: tobj = clause Ask.ask c.0(e.1, ())
  decref x.3
  return k.2
}
";
    assert_eq!(points_of(text, "f"), [point(0, 0, &[(2, 1)], None)]);
}

/// 所有の段の verifier が拒む関数は、地点の代わりにその誤りを返す。VM は、その関数を表のない関数として扱う。
#[test]
fn unverified_lir_has_no_points() {
    let program = parse("fn f(a.0: obj) -> unit {\n  return ()\n}\n")
        .unwrap_or_else(|error| panic!("{error}"));
    let error = points(&program, |_| true)
        .swap_remove(0)
        .expect("every function is wanted")
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "`a.0` is still owned at the end of the function in `f`"
    );
}

#[test]
fn a_live_token_is_listed_apart_from_owned() {
    // token は RC の対象でないので `owned` に入れず、巻き戻しで `drop_token` するものとして別に並べる
    let text = "\
layout P { P(obj, obj) }
fn g(s.0: obj) -> obj {
  let z.1: obj = extern Prelude.exit(1)
  decref s.0
  return z.1
}
fn f(x.0: obj) -> obj {
  unpack x.0 P #0(a.1: obj, b.2: obj)
  let r.3: token = release x.0 P #0(a.1, b.2)
  let z.4: obj = call g(a.1)
  let d.5: obj = con P #0(z.4, b.2) reuse r.3
  return d.5
}
";
    assert_eq!(
        points_of(text, "f"),
        [UnwindPoint {
            tokens: vec![VarId(3)],
            ..point(0, 2, &[(2, 1)], None)
        }]
    );
}

/// 中断しない関数の呼び出しは、巻き戻しの地点にならない (docs/spec/lir.md の「巻き戻しの地点」)。
#[test]
fn call_to_pure_function_is_not_a_point() {
    let text = "\
fn g(x.0: obj) -> obj {
  return x.0
}
fn f(a.0: obj, b.1: obj) -> obj {
  let r.2: obj = call g(a.0)
  decref r.2
  return b.1
}
";
    assert_eq!(points_of(text, "f"), []);
    assert!(!aborts(text, "g"));
    assert!(!aborts(text, "f"));
}

#[test]
fn call_to_function_that_calls_an_effectful_extern_is_a_point() {
    let text = "\
fn g(x.0: obj) -> obj {
  dup x.0
  let u.1: unit = extern Prelude.println(x.0)
  return x.0
}
fn f(a.0: obj, b.1: obj) -> obj {
  let r.2: obj = call g(a.0)
  decref r.2
  return b.1
}
";
    assert_eq!(points_of(text, "f"), [point(0, 0, &[(1, 1)], None)]);
    assert!(aborts(text, "g"));
    assert!(aborts(text, "f"));
}

/// 末尾の呼び出しも中断を伝える。`f` は `g` を末尾で呼ぶだけだが、`g` の `+` が `Panic` を起こしうるので、`f` を
/// 末尾でなく呼ぶ `h` の呼び出しは地点である。
#[test]
fn tail_call_propagates_abort() {
    let text = "\
fn g(n.0: int) -> int {
  let m.1: int = extern \"Prelude.Num Int.+\"(n.0, 1)
  return m.1
}
fn f(n.0: int) -> int {
  tail call g(n.0)
}
fn h(n.0: int, s.1: obj) -> obj {
  let m.2: int = call f(n.0)
  return s.1
}
";
    assert!(aborts(text, "f"));
    assert_eq!(points_of(text, "f"), []);
    assert_eq!(points_of(text, "h"), [point(0, 0, &[(1, 1)], None)]);
}

/// 最小の不動点なので、中断する文を持たない相互再帰は中断しない。
#[test]
fn recursive_pure_functions_do_not_abort() {
    let text = "\
fn even(n.0: int) -> int {
  let r.1: int = call odd(n.0)
  return r.1
}
fn odd(n.0: int) -> int {
  let r.1: int = call even(n.0)
  return r.1
}
";
    assert!(!aborts(text, "even"));
    assert!(!aborts(text, "odd"));
    assert_eq!(points_of(text, "even"), []);
    assert_eq!(points_of(text, "odd"), []);
}

/// `apply` は、呼ぶ関数が中断しなくても地点である。すぐに再開する節は操作を起こしたフレームの上でその場で動くので、
/// どの閉包の呼び出しにも中断が通りうる (docs/spec/lir.md の「巻き戻しの地点」)。末尾の `apply` も関数を中断しうる
/// ものにする。
#[test]
fn apply_is_always_a_point() {
    let text = "\
fn g(x.0: tobj) -> tobj {
  return x.0
}
fn f(x.0: tobj, s.1: obj) -> obj {
  let r.3: tobj = apply &g(x.0)
  decref r.3
  return s.1
}
fn h(c.0: tobj) -> tobj {
  tail apply c.0(())
}
";
    assert_eq!(points_of(text, "f"), [point(0, 0, &[(1, 1)], None)]);
    assert!(!aborts(text, "g"));
    assert!(aborts(text, "f"));
    assert!(aborts(text, "h"));
}

#[test]
fn leaves_of_a_multi_value_call_are_not_owned() {
    // 複数の値を返す呼び出しの結果は、呼び出しが戻らなければまだないので、どの葉も数えない
    // (docs/spec/vm.md の「巻き戻しの表」)。`f` が中断しうるのは
    // この呼び出しだけによる
    let text = "\
fn g(x.0: obj) -> (obj, obj) {
  let z.1: obj = extern Prelude.exit(1)
  return (x.0, z.1)
}
fn f(a.0: obj, b.1: obj) -> obj {
  let (p.2: obj, q.3: obj) = call g(a.0)
  decref p.2
  decref q.3
  return b.1
}
";
    assert!(aborts(text, "f"));
    assert_eq!(points_of(text, "f"), [point(0, 0, &[(1, 1)], None)]);
}

#[test]
fn par_and_child_abort_are_points() {
    // `par` は子の中断を親で起こし直し、`child_abort` は子を中断するので、どちらも呼ぶ相手によらず地点になる。
    // `child_abort` が借りる evidence は、命令の後も持ち主が持っている
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn f(a.0: tobj, b.1: tobj, s.2: obj) -> obj {
  let t.3: obj = par (,) #0(a.0, b.1)
  decref t.3
  return s.2
}
fn g(e.0: obj, c.1: tobj poly) -> obj poly {
  let u.2: unit = prim child_abort(e.0, c.1)
  decref e.0
  let d.3: obj poly = prim dummy()
  return d.3
}
";
    assert_eq!(points_of(text, "f"), [point(0, 0, &[(2, 1)], None)]);
    assert_eq!(points_of(text, "g"), [point(0, 0, &[(0, 1)], None)]);
    assert!(aborts(text, "f"));
    assert!(aborts(text, "g"));
}
