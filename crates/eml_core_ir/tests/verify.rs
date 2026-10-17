//! Core IR のテキストで書いた IR で、verifier が正しいものを受け入れ、壊れたものを拒むことを確かめる (docs/spec/core-ir.md)。
//! 構造の規則 R1 から R8 と、前の形の IR から引き継いだ検査を1つずつ確かめる。

use eml_core_ir::{Program, Rhs, Stmt, parse, verify_evidence, verify_translated};
use eml_lir::{MaskItem, Prim};

fn read(text: &str) -> Program {
    parse(text).unwrap_or_else(|error| panic!("{error}"))
}

fn check_translated(text: &str) -> Result<(), String> {
    verify_translated(&read(text)).map_err(|error| error.to_string())
}

fn check_evidence(text: &str) -> Result<(), String> {
    verify_evidence(&read(text)).map_err(|error| error.to_string())
}

/// `k n = n`。
const K: &str = "fn k(a.0: int) -> int {\n  return a.0\n}\n";

const ASK: &str = "effect Ask { ask(tobj) -> tobj }\n";

/// `handle ask 1 with | ctl ask x k -> resume k x` を持ち上げた translate の出力の形。`effects` は先頭のエフェクトの行。
/// box の挿入の前なので、捕まえる `Int` と handle の結果は `int` のままである。
fn handler_program(effects: &str) -> String {
    format!(
        "{effects}fn main() -> int {{
  let c.0: tobj = closure main$handle0(1)
  let c.1: tobj = closure main$handle0$ask(2)
  let r.2: int = handle Ask((), c.0) {{ ask: c.1 }} return &main$handle0$return
  return r.2
}}
fn main$handle0(n.0: int, p.1: unit) -> tobj {{
  let t.2: tobj = perform Ask.ask(n.0)
  return t.2
}}
fn main$handle0$ask(m.0: int, x.1: tobj, k.2: tobj, s.3: unit) -> tobj {{
  let t.4: tobj = resume k.2(x.1, s.3)
  return t.4
}}
fn main$handle0$return(x.0: tobj, s.1: unit) -> tobj {{
  return x.0
}}
"
    )
}

// R7

/// `obj` の値と `unit` の値と `()` を、`tobj` の引数に渡す。
fn compatible_jump() -> &'static str {
    "fn f(s.0: obj, u.1: unit) -> tobj {
  jump b1(s.0, u.1, ())
b1(a.2: tobj, b.3: tobj, c.4: tobj):
  return a.2
}
"
}

#[test]
fn a_jump_passes_values_to_compatible_parameters() {
    // `obj` と `tobj`、`unit` と `tobj` は互換で、命令なしで行き来する (docs/spec/lir.md の「値の表現」)
    assert_eq!(check_translated(compatible_jump()), Ok(()));
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
    assert_eq!(check_translated(text), Ok(()));
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
        assert_eq!(check_translated(text), Err(message.to_string()));
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
    assert_eq!(check_evidence(text), Ok(()));
    // translate の出力の `never` の `perform` の束縛も、`return` と `jump` で位置と比べない
    let translated = "\
effect Fail { never fail(tobj) -> never }
fn f(s.0: obj) -> tobj {
  let t.1: int = perform never Fail.fail(s.0)
  return t.1
}
fn g(s.0: obj) -> tobj {
  let t.1: int = perform never Fail.fail(s.0)
  jump b1(t.1)
b1(a.2: tobj):
  return a.2
}
";
    assert_eq!(check_translated(translated), Ok(()));
}

// 境界の検査

#[test]
fn the_translated_level_allows_a_function_value_that_is_not_uniform() {
    // translate は `if c then double else inc` を `jump b3(&double)` にする。一様にするのは box の挿入である
    let text = format!("fn f() -> tobj {{\n  jump b1(&k)\nb1(h.0: tobj):\n  return h.0\n}}\n{K}");
    assert_eq!(check_translated(&text), Ok(()));
}

#[test]
fn the_translated_level_allows_a_dropped_or_switched_function_value_that_is_not_uniform() {
    // 値を呼び出しに渡さない `drop` と case のない `switch` でも、`&k` は関数の値である
    for body in [
        "  let u.0: unit = drop &k\n  return u.0\n",
        "  switch &k { _ -> b1 }\nb1:\n  return ()\n",
    ] {
        let text = format!("fn f() -> unit {{\n{body}}}\n{K}");
        assert_eq!(check_translated(&text), Ok(()));
    }
}

// 借りたフィールド

#[test]
fn what_perceus_gives_a_target_that_uses_its_scrutinee_is_accepted() {
    // `label` はフィールドのない行き先と、フィールドを使う行き先の両方で、scrutinee を後でも使う。`rest` は
    // `| Nil -> xs` の形で、フィールドのない行き先が scrutinee をそのまま返す
    let text = "\
data Option a =
  | None
  | Some a

data List a =
  | Nil
  | Cons a (List a)

show : Option String -> String
show o = match o with
  | Some s -> s
  | None -> \"none\"

label : Option String -> String
label o =
  let first = match o with
    | Some s -> s ++ show o
    | None -> show o
  first ++ show o

rest : List Int -> List Int
rest xs = match xs with
  | Nil -> xs
  | Cons _ t -> t

size : List Int -> Int
size xs = match xs with
  | Nil -> 0
  | Cons _ t -> 1 + size t

main : Unit -> <IO> Unit
main () =
  println (label (Some \"a\"))
  println (Prelude.show (size (rest (Cons 1 Nil))))
";
    assert_eq!(eml_lir::verify(&eml_test_support::lir(text)), Ok(()));
}

// switch

#[test]
fn a_switch_with_two_defaults_does_not_parse() {
    let text = "\
fn main(n.0: int) -> int {
  switch n.0 { 1 -> b1, _ -> b2, _ -> b3 }
b1:
  return 10
b2:
  return 0
b3:
  return 1
}
";
    let error = parse(text).unwrap_err().to_string();
    assert!(error.contains("two defaults"), "{error}");
}

// エフェクト
//
// `handle`、`perform`、`resume`、`mask` は translate の出力にだけ現れる。EP がそれを消すので、この規則は変換の段で
// 確かめる

#[test]
fn a_handler_has_a_clause_for_each_operation() {
    assert_eq!(check_translated(&handler_program(ASK)), Ok(()));
    assert_eq!(
        check_translated(&handler_program(
            "effect Ask { ask(tobj) -> tobj, tell(tobj) -> tobj }\n"
        )),
        Err(
            "a handler of `Ask` has clauses for 1 operations, but the effect has 2 in `main`"
                .to_string()
        )
    );
}

#[test]
fn a_clause_receives_the_arguments_k_and_the_state_after_its_captures() {
    let text = "\
effect Ask { ask(tobj) -> tobj }
fn f() -> int {
  let t.0: int = handle Ask((), &body) { ask: &clause } return &ret
  return t.0
}
fn body(u.0: unit) -> int {
  return 1
}
fn clause(x.0: int, k.1: tobj) -> int {
  let t.2: int = resume k.1(x.0, ())
  return t.2
}
fn ret(x.0: int, s.1: unit) -> int {
  return x.0
}
";
    assert_eq!(
        check_translated(text),
        Err("the clause for `ask` needs 3 parameters after its captures (1 for the arguments, `k`, and the state), but it has 2 in `f`".to_string())
    );
}

#[test]
fn a_clause_of_a_never_operation_receives_the_arguments_and_the_state() {
    let text = "\
effect Fail { never fail(tobj) -> never }
fn f(n.0: tobj) -> int {
  let c.1: tobj = closure clause(n.0)
  let t.2: int = handle Fail((), &body) { fail: c.1 } return &ret
  return t.2
}
fn body(u.0: unit) -> int {
  return 1
}
fn clause(n.0: tobj, x.1: tobj, k.2: tobj, s.3: unit) -> tobj {
  return n.0
}
fn ret(x.0: int, s.1: unit) -> int {
  return x.0
}
";
    assert_eq!(
        check_translated(text),
        Err("the clause for `fail` needs 2 parameters after its captures (1 for the arguments and the state), but it has 3 in `f`".to_string())
    );
}

#[test]
fn a_return_clause_receives_the_value_and_the_state_after_its_captures() {
    let text = "\
effect Ask { ask(tobj) -> tobj }
fn f(n.0: tobj) -> int {
  let c.1: tobj = closure ret(n.0)
  let t.2: int = handle Ask((), &body) { ask: &clause } return c.1
  return t.2
}
fn body(u.0: unit) -> int {
  return 1
}
fn clause(x.0: int, k.1: tobj, s.2: unit) -> int {
  let t.3: int = resume k.1(x.0, s.2)
  return t.3
}
fn ret(n.0: tobj, x.1: tobj) -> tobj {
  return x.1
}
";
    assert_eq!(
        check_translated(text),
        Err("the `return` clause needs 2 parameters after its captures (the value and the state), but it has 1 in `f`".to_string())
    );
}

#[test]
fn a_clause_outside_its_scope_is_rejected_before_its_parameters_are_counted() {
    // `c.1` は b1 だけで定義され、`handle` のある b3 を支配しない。引数の数も合わないが、先に範囲の誤りを報告する
    let text = "\
layout Prelude.Bool { False, True }
effect Ask { ask(tobj) -> tobj }
fn f(k.0: enum) -> int {
  switch k.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let c.1: tobj = closure g(())
  jump b3()
b2:
  jump b3()
b3:
  let t.2: int = handle Ask((), &body) { ask: c.1 } return &ret
  return t.2
}
fn g(y.0: unit, x.1: tobj) -> tobj {
  return x.1
}
fn body(u.0: unit) -> int {
  return 1
}
fn ret(x.0: int, s.1: unit) -> int {
  return x.0
}
";
    assert_eq!(
        check_translated(text),
        Err("`c.1` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn perform_names_an_operation_of_its_effect() {
    let text =
        format!("{ASK}fn f() -> tobj {{\n  let t.0: tobj = perform Ask.#1()\n  return t.0\n}}\n");
    assert_eq!(
        check_translated(&text),
        Err("`perform` names operation 1 of `Ask`, which has 1 operations in `f`".to_string())
    );
}

#[test]
fn a_perform_must_agree_with_the_effect_on_resuming() {
    let resumes = "effect Fail { never fail(tobj) -> never }\nfn f() -> tobj {\n  let t.0: tobj = perform Fail.fail(())\n  return t.0\n}\n";
    assert_eq!(
        check_translated(resumes),
        Err("`Fail.fail` never resumes, but this perform resumes in `f`".to_string())
    );
    let never = "effect Ask { ask(tobj) -> tobj }\nfn f() -> tobj {\n  let t.0: tobj = perform never Ask.ask(1)\n  return t.0\n}\n";
    assert_eq!(
        check_translated(never),
        Err("`Ask.ask` resumes, but this perform never resumes in `f`".to_string())
    );
}

/// テキストは `handle` と `perform` の前の `mask` を読まないので、読んだ後で `mask` を付ける。
#[test]
fn a_mask_on_handle_or_perform_is_rejected() {
    let texts = [
        "effect Ask { ask(tobj) -> tobj }\nfn f(b.0: tobj, c.1: tobj, r.2: tobj) -> tobj {\n  let t.3: tobj = handle Ask((), b.0) { ask: c.1 } return r.2\n  return t.3\n}\n",
        "effect Ask { ask(tobj) -> tobj }\nfn f() -> tobj {\n  let t.0: tobj = perform Ask.ask(1)\n  return t.0\n}\n",
    ];
    let errors: Vec<String> = texts
        .into_iter()
        .map(|text| {
            let mut program = read(text);
            let Stmt::Let {
                var: _,
                rhs: Rhs::Call { mask, .. },
            } = &mut program.functions[0].blocks[0].stmts[0]
            else {
                panic!("not a call");
            };
            mask.push(MaskItem {
                effect: 0,
                keep: 0,
                skip: 1,
            });
            verify_translated(&program).unwrap_err().message
        })
        .collect();
    assert_eq!(errors, ["a mask on handle", "a mask on perform"]);
}

/// テキストは `clause` の前の `mask` を読まないので、読んだ後で `mask` を付ける。`mask` の形は変換の段だけが確かめ、
/// 後の段は `mask` が残ったこと自体を拒む。
#[test]
fn a_mask_on_a_clause_is_rejected() {
    let mut program = read(&format!(
        "{COUNTER}fn f(c.0: tobj, e.1: obj) -> int {{\n  let t.2: int = clause Counter.get c.0(e.1, ())\n  return t.2\n}}\n"
    ));
    let Stmt::Let {
        var: _,
        rhs: Rhs::Call { mask, .. },
    } = &mut program.functions[0].blocks[0].stmts[0]
    else {
        panic!("not a call");
    };
    mask.push(MaskItem {
        effect: 0,
        keep: 0,
        skip: 1,
    });
    assert_eq!(
        verify_translated(&program).unwrap_err().message,
        "a mask on clause"
    );
    assert_eq!(
        verify_evidence(&program).unwrap_err().message,
        "a mask remains after the evidence pass"
    );
}

#[test]
fn a_mask_must_name_known_effects_in_order() {
    let unknown = "\
effect Main.State { get(tobj) -> tobj, put(tobj) -> tobj }
fn entry$main(c.0: tobj) -> tobj {
  let t.1: tobj = mask [#5] apply c.0(())
  return t.1
}
";
    let unordered = "\
effect Main.A { a(tobj) -> tobj }
effect Main.B { b(tobj) -> tobj }
fn entry$main(c.0: tobj) -> tobj {
  let t.1: tobj = mask [Main.B, Main.A] apply c.0(())
  return t.1
}
";
    assert_eq!(
        check_translated(unknown),
        Err("a mask names an unknown effect #5 in `entry$main`".to_string())
    );
    assert_eq!(
        check_translated(unordered),
        Err("a mask is not in ascending order in `entry$main`".to_string())
    );
}

#[test]
fn a_mask_names_each_effect_once() {
    let text = "\
effect Main.A { a(tobj) -> tobj }
fn entry$main(c.0: tobj) -> tobj {
  let t.1: tobj = mask [Main.A, Main.A@1] apply c.0(())
  return t.1
}
";
    assert_eq!(
        check_translated(text),
        Err("a mask is not in ascending order in `entry$main`".to_string())
    );
}

#[test]
fn a_mask_item_skips_some_handler() {
    let text = "\
effect Main.A { a(tobj) -> tobj }
fn entry$main(c.0: tobj) -> tobj {
  let t.1: tobj = mask [Main.A] apply c.0(())
  return t.1
}
";
    let mut program = read(text);
    let Stmt::Let {
        var: _,
        rhs: Rhs::Call { mask, .. },
    } = &mut program.functions[0].blocks[0].stmts[0]
    else {
        panic!("not a call");
    };
    mask[0].skip = 0;
    assert_eq!(
        verify_translated(&program).unwrap_err().message,
        "a mask item of `Main.A` skips no handler"
    );
}

#[test]
fn a_mask_may_skip_one_effect_twice() {
    let text = "\
effect Main.A { a(tobj) -> tobj }
effect Main.B { b(tobj) -> tobj }
fn entry$main(c.0: tobj) -> tobj {
  let t.1: tobj = mask [Main.A*2, Main.B] apply c.0(())
  let t.2: tobj = mask [Main.B] apply t.1(())
  return t.2
}
";
    assert_eq!(check_translated(text), Ok(()));
}

// 大きさ

#[test]
fn a_long_run_of_if_statements_is_verified_in_linear_time() {
    // 文の `if` が続くと、`switch` と合流するブロックが文の数だけ連なる。`lower` はデバッグビルドで毎回 verify するので、
    // 文の数に比例する時間で終わらなければならない。条件は引数にする。`if True` は translate がその場で枝を選び、
    // `switch` を出さないためである
    // IO の extern の呼び出しは yield の地点なので (docs/spec/core-ir.md の「IO の extern」)、`println` ごとに
    // yield の確認と遅い道が足される。文の数を 10000 にとどめ、並列に走るほかのテストで遅くなっても上限との間に余裕を残す。
    // 2乗の verifier なら 10000 でも上限を大きく超える。時間の比 (2N と N) で確かめる形は、2回の計測がどちらも負荷で揺れるので採らない
    const COUNT: usize = 10000;
    let mut text = String::from("step : Bool -> <IO> Unit\nstep b =\n  let s = \"keep\"\n");
    text.push_str(&"  if b then println \"x\"\n".repeat(COUNT));
    text.push_str("  println s\n\nmain : Unit -> <IO> Unit\nmain () = step True");
    let start = std::time::Instant::now();
    let program = eml_test_support::lir(&text);
    assert_eq!(eml_lir::verify(&program), Ok(()));
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "took {elapsed:?}"
    );
    let step = program
        .functions
        .iter()
        .find(|function| function.name == "step")
        .expect("step");
    assert_eq!(crate::common::if_switches(step), COUNT);
}

// evidence の段と、基本操作と節の呼び出し

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
    assert_eq!(check_evidence(EVIDENCE_EXAMPLE), Ok(()));
}

/// EP の後の段が拒むエフェクトの命令と、その文言。
fn effect_instructions() -> [(String, &'static str); 4] {
    [
        (
            format!(
                "{ASK}fn f() -> tobj {{\n  let t.0: tobj = perform Ask.ask(())\n  return t.0\n}}\n"
            ),
            "a perform remains after the evidence pass in `f`",
        ),
        (
            format!(
                "{ASK}fn f(b.0: tobj, c.1: tobj, r.2: tobj) -> tobj {{\n  let t.3: tobj = handle Ask((), b.0) {{ ask: c.1 }} return r.2\n  return t.3\n}}\n"
            ),
            "a handler remains after the evidence pass in `f`",
        ),
        (
            "fn f(k.0: tobj) -> tobj {\n  let t.1: tobj = resume k.0((), ())\n  return t.1\n}\n"
                .to_string(),
            "a resume remains after the evidence pass in `f`",
        ),
        (
            format!(
                "{ASK}fn f(c.0: tobj) -> tobj {{\n  let t.1: tobj = mask [Ask] apply c.0(())\n  return t.1\n}}\n"
            ),
            "a mask remains after the evidence pass in `f`",
        ),
    ]
}

#[test]
fn the_evidence_level_rejects_effect_instructions() {
    for (text, message) in effect_instructions() {
        assert_eq!(check_evidence(&text), Err(message.to_string()));
    }
}

#[test]
fn the_translated_level_rejects_prims_and_clauses() {
    assert_eq!(
        check_translated("fn f() -> tobj {\n  let w.0: tobj = prim evv_get()\n  return w.0\n}\n"),
        Err("`prim evv_get` is used before the evidence pass in `f`".to_string())
    );
    assert_eq!(
        check_translated(&format!(
            "{COUNTER}fn f(c.0: tobj, e.1: obj) -> int {{\n  let t.2: int = clause Counter.get c.0(e.1, ())\n  return t.2\n}}\n"
        )),
        Err("a clause call is used before the evidence pass in `f`".to_string())
    );
    assert_eq!(
        check_translated(&format!(
            "{COUNTER}fn f() -> int {{\n  return 1\n}}\ninternal clause Counter.get fn g(e.0: obj, u.1: unit) -> int {{\n  return 1\n}}\n"
        )),
        Err("a clause of `Counter.get` is defined before the evidence pass in `g`".to_string())
    );
}

/// 根の基本操作は translate が作るので、evidence passing の前の段でも受け入れる (docs/spec/lir.md の「基本操作」)。
#[test]
fn the_translated_level_accepts_root_prims() {
    let text = "fn f(x.0: tobj) -> tobj {\n  let u.1: unit = prim root_set[0](x.0)\n  let y.2: tobj = prim root_get[0]()\n  return y.2\n}\n";
    assert_eq!(check_translated(text), Ok(()));
    assert_eq!(check_evidence(text), Ok(()));
}

#[test]
fn a_prim_takes_its_arguments() {
    assert_eq!(
        check_evidence("fn f() -> unit {\n  let u.0: unit = prim evv_set()\n  return u.0\n}\n"),
        Err("`prim evv_set` takes 1 arguments but is given 0 in `f`".to_string())
    );
}

#[test]
fn a_prim_names_known_effects_and_operations() {
    assert_eq!(
        check_evidence("fn f() -> obj {\n  let e.0: obj = prim ev_lookup[#2]()\n  return e.0\n}\n"),
        Err(
            "`prim ev_lookup[#2]` names effect 2, which is not in the effect table in `f`"
                .to_string()
        )
    );
    assert_eq!(
        check_evidence(&format!(
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
    assert_eq!(check_evidence(&mask("State*2, Counter")), Ok(()));
    assert_eq!(
        check_evidence(&mask("Counter, State")),
        Err(
            "the effects of `prim evv_mask[Counter, State]` are not in ascending order in `f`"
                .to_string()
        )
    );
    assert_eq!(
        check_evidence(&mask("State, #4")),
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
        verify_evidence(&program).unwrap_err().message,
        "`prim evv_mask[]` masks no effect"
    );
}

/// 表にないエフェクトの操作の印は、操作の番号より先にエフェクトを報告する。
#[test]
fn a_clause_function_names_a_known_effect() {
    assert_eq!(
        check_evidence(&format!(
            "{COUNTER}fn f() -> int {{\n  return 1\n}}\ninternal clause #3.#0 fn g(e.0: obj) -> unit {{\n  return ()\n}}\n"
        )),
        Err("`g` is a clause of effect 3, which is not in the effect table in `g`".to_string())
    );
}

#[test]
fn a_clause_call_names_an_operation_and_takes_its_arguments() {
    assert_eq!(
        check_evidence(&format!(
            "{COUNTER}fn f(c.0: tobj, e.1: obj) -> unit {{\n  let u.2: unit = clause Counter.#5 c.0(e.1, 1)\n  return u.2\n}}\n"
        )),
        Err("a clause names operation 5 of `Counter`, which has 2 operations in `f`".to_string())
    );
    assert_eq!(
        check_evidence(&format!(
            "{COUNTER}fn f(c.0: tobj, e.1: obj) -> unit {{\n  let u.2: unit = clause Counter.put c.0(e.1)\n  return u.2\n}}\n"
        )),
        Err("a clause of `Counter.put` is given 1 arguments, but it takes 2 (the evidence and 1 arguments of the operation) in `f`".to_string())
    );
}

#[test]
fn the_evidence_level_does_not_compare_boundaries() {
    // box の挿入の前なので、Repr の食い違いは box の挿入が変換で埋める
    let text = format!(
        "{COUNTER}fn f(c.0: tobj, n.1: tobj, e.2: obj) -> tobj {{\n  let u.3: unit = clause Counter.put c.0(e.2, n.1)\n  return u.3\n}}\n"
    );
    assert_eq!(check_evidence(&text), Ok(()));
}

#[test]
fn a_clause_function_has_the_reprs_of_its_operation() {
    // 引数の数は境界の検査でないので、evidence の段でも確かめる
    assert_eq!(
        check_evidence(&format!(
            "{COUNTER}fn f() -> int {{\n  return 1\n}}\ninternal clause Counter.put fn g(n.0: int) -> unit {{\n  return ()\n}}\n"
        )),
        Err("`g` is a clause of `Counter.put`, so it takes the evidence and 1 arguments after its captures, but it has 1 parameters in `g`".to_string())
    );
    assert_eq!(
        check_evidence(&format!(
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
        check_evidence(&format!(
            "{COUNTER}fn f(c.0: tobj) -> tobj {{\n  let t.1: tobj = apply c.0(&g)\n  return t.1\n}}\n{PUT_CLAUSE}"
        )),
        used_as_value
    );
    assert_eq!(
        check_evidence(&format!(
            "{COUNTER}fn f(m.0: tobj, c.1: tobj) -> tobj {{\n  let g.2: tobj = closure g(m.0)\n  let t.3: tobj = apply c.1(g.2)\n  return t.3\n}}\n{PUT_CLAUSE}"
        )),
        used_as_value
    );
    assert_eq!(
        check_evidence(&format!(
            "{COUNTER}fn f() -> tobj {{\n  return &g\n}}\n{PUT_CLAUSE}"
        )),
        used_as_value
    );
    assert_eq!(
        check_evidence(&format!(
            "{COUNTER}fn f(m.0: tobj, e.1: obj) -> unit {{\n  let u.2: unit = call g(m.0, e.1, 1)\n  return u.2\n}}\n{PUT_CLAUSE}"
        )),
        Err("`g` is a clause of `Counter.put`, but it is called directly in `f`".to_string())
    );
    // 捕獲のある節は、同じ関数の中の `closure` で入口の表に入れる
    let put = format!(
        "{COUNTER}fn f(m.0: tobj, w.1: tobj) -> unit {{\n  let e.2: obj = prim ev_new[Counter]((), w.1)\n  let g.3: tobj = closure g(m.0)\n  let u.4: unit = prim ev_set_clause[Counter.put](e.2, g.3)\n  return u.4\n}}\n{PUT_CLAUSE}"
    );
    assert_eq!(check_evidence(&put), Ok(()));
}

#[test]
fn ev_set_clause_names_the_clause_of_its_function() {
    let set = |entry: &str, value: &str| {
        check_evidence(&format!(
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

// 印 (docs/spec/lir.md の「Kind の印の検査」)

#[test]
fn v2_runs_only_on_jump_and_return_at_the_core_ir_levels() {
    let both = |text: &str| {
        let translated = check_translated(text);
        assert_eq!(check_evidence(text), translated, "{text}");
        translated
    };
    // 呼び出しの引数、extern の引数と結果は、box の挿入の後の LIR の段で比べる
    let calls = "\
fn f(s.0: obj) -> unit {
  let h.1: obj = extern Std.Fs.open(s.0)
  let u.2: unit = call g(h.1)
  return u.2
}
fn g(h.0: obj lin) -> unit {
  let u.1: unit = extern Std.Fs.close(h.0)
  return u.1
}
";
    assert_eq!(both(calls), Ok(()));
    assert_eq!(
        both("fn f(s.0: obj) -> obj lin {\n  jump b1(s.0)\nb1(t.1: obj lin):\n  return t.1\n}\n"),
        Err("a jump to b1 passes `s.0` (unr) to `t.1` (lin) in `f`".to_string())
    );
    assert_eq!(
        both("fn f(s.0: obj lin) -> obj {\n  return s.0\n}\n"),
        Err("`s.0` (lin) is returned from a function that returns unr in `f`".to_string())
    );
    // `poly` はどちらとも、定数はどの印とも合う
    assert_eq!(
        both(
            "fn f(s.0: obj lin) -> tobj lin {\n  jump b1(s.0, ())\nb1(t.1: tobj poly, u.2: unit lin):\n  return t.1\n}\n"
        ),
        Ok(())
    );
}

#[test]
fn the_build_check_runs_at_the_core_ir_levels() {
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn f(h.0: obj lin) -> obj {
  let p.1: obj = con (,) #0(h.0, ())
  return p.1
}
";
    let message = Err(
        "`p.1` (unr) is bound to a con of `(,)` #0, whose arguments join to lin in `f`".to_string(),
    );
    assert_eq!(check_translated(text), message);
    assert_eq!(check_evidence(text), message);
    let closure = "\
fn f(h.0: obj lin) -> tobj {
  let c.1: tobj = closure g(h.0)
  return c.1
}
fn g(h.0: obj lin, u.1: unit) -> tobj lin {
  return h.0
}
";
    assert_eq!(
        check_translated(closure),
        Err(
            "`c.1` (unr) is bound to a closure of `g`, whose captures join to lin in `f`"
                .to_string()
        )
    );
}

// 着地の印 `prompt v`

fn marked(body: &str, ev: &str, ret: &str) -> String {
    format!("fn f(b.0: tobj, {ev}) -> {ret} {{\n{body}}}\n")
}

#[test]
fn prompt_marker_is_accepted_after_the_evidence_pass() {
    let text = marked(
        "  let x.2: tobj = yields apply b.0(()) prompt ev.1\n  return ev.1\n",
        "ev.1: obj",
        "obj",
    );
    assert_eq!(check_evidence(&text), Ok(()));
}

#[test]
fn prompt_marker_is_rejected_before_the_evidence_pass() {
    let text = marked(
        "  let x.2: tobj = yields apply b.0(()) prompt ev.1\n  return ev.1\n",
        "ev.1: obj",
        "obj",
    );
    assert_eq!(
        check_translated(&text),
        Err("a prompt marker appears before the evidence pass in `f`".to_string())
    );
}

#[test]
fn prompt_marker_in_tail_position_is_rejected() {
    let text = marked(
        "  let x.2: tobj = yields apply b.0(()) prompt ev.1\n  return x.2\n",
        "ev.1: obj",
        "tobj",
    );
    assert_eq!(
        check_evidence(&text),
        Err("an apply with the prompt marker `ev.1` is in a tail position, which leaves no frame to land on in `f`".to_string())
    );
}

#[test]
fn prompt_marker_on_a_dead_variable_is_rejected() {
    let text = marked(
        "  let x.2: tobj = yields apply b.0(()) prompt ev.1\n  return ()\n",
        "ev.1: obj",
        "unit",
    );
    assert_eq!(
        check_evidence(&text),
        Err(
            "`ev.1` is the prompt marker of an apply, but it is not used after the call in `f`"
                .to_string()
        )
    );
}

#[test]
fn prompt_marker_on_a_non_obj_variable_is_rejected() {
    let text = marked(
        "  let x.2: tobj = yields apply b.0(()) prompt ev.1\n  return ev.1\n",
        "ev.1: tobj",
        "tobj",
    );
    assert_eq!(
        check_evidence(&text),
        Err("`ev.1` is the prompt marker of an apply, but it is tobj, not obj in `f`".to_string())
    );
}

#[test]
fn prompt_marker_used_only_in_a_sibling_branch_is_rejected() {
    let text = "layout Prelude.Bool { False, True }\nfn f(b.0: tobj, ev.1: obj, c.2: enum, e.3: obj) -> obj {
  switch c.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let x.4: tobj = yields apply b.0(()) prompt ev.1
  return e.3
b2:
  return ev.1
}
";
    assert_eq!(
        check_evidence(text),
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
    assert_eq!(check_evidence(text), Ok(()));
}
