//! box の挿入のパス (docs/spec/lir.md の「box の挿入」)。ソースから `eml_lir::Pass::Boxing` までを通し、縮約の
//! 前の LIR を見る。末尾呼び出しを確かめるテストは `eml_lir::Pass::Contract` まで通す。手で書いた LIR を入力にする
//! テストは `eml_lir` の tests/boxing.rs にある。

use crate::common::{function, lir_text};
use eml_lir::{Call, FnIdx, Rhs, Stmt, Term};

const APPLY_TO: &str =
    "apply_to : (Int -> <Panic> Int) -> Int -> <Panic> Int\napply_to f x = f x\n\n";

/// 関数の値をデータに入れて取り出す。optimize はデータを通った値を既知の閉包としないので
/// (docs/spec/core-ir.md の「既知の閉包」)、取り出した値への呼び出しは
/// 特殊化されず、値として使う関数の形を確かめられる。調べる関数をこの形で呼ぶと、呼ぶ側に展開されず (「インライン化」)、
/// 自分のコードのまま残る。`hide` は自分を呼ぶので展開されず、取り出した値は呼ぶ側から見えない。
const HIDE: &str = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\n";

/// 値としてだけ使うラムダも型どおりの ABI を持つ。`apply` が実行時に Repr を合わせるので、一様にしない
/// (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「関数の ABI」)。
#[test]
fn a_lambda_used_only_as_a_value_keeps_its_typed_abi() {
    let text = format!(
        "{HIDE}{APPLY_TO}main : Unit -> <IO, Panic> Unit\nmain () =\n  let Box f = hide 0 (fn n -> n + 1)\n  println (show (apply_to f 2))"
    );
    let shown = lir_text(&text, eml_lir::Pass::Boxing);
    insta::assert_snapshot!(function(&shown, "main$lambda0"), @r#"
    internal fn main$lambda0(n.0: int) -> int {
      let t.1: int = extern "Prelude.Num Int.+"(n.0, 1)
      return t.1
    }
    "#);
}

/// 複数の定義が値として使う、translate の共有の関数の名前の接頭辞。
fn is_shared_wrapper(name: &str) -> bool {
    ["op$", "con$", "cont$"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

#[test]
fn shared_wrappers_are_never_called_directly() {
    // `op$`、`con$`、`cont$` は値と部分適用でだけ使われ、直接は呼ばれない。どのプログラムでも同じコードになるので、
    // 名前で照らす追記で使い回せる (docs/spec/core-ir.md の「REPL の入力」)
    let text = "data Pair = | P Int Int\n\ndata Box a = | Box a\n\neffect Ask where\n  ask : Int -> Int\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\nfirst : Pair -> Int\nfirst p = match p with\n  | P a _ -> a\n\nrun : Unit -> <Ask, Panic> Int\nrun () =\n  let Box g = hide 0 ask\n  let Box f = hide 0 P\n  let Box h = hide 0 (P 3)\n  first (f 1 2) + first (h 4) + g 1\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let r = handle run () with\n            | ctl ask n k ->\n              let Box resume = hide 0 k\n              resume (n + 1)\n  println (show r)";
    let program = eml_test_support::lir(text);
    let names: Vec<&str> = program
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .filter(|name| is_shared_wrapper(name))
        .collect();
    assert_eq!(names, ["op$ask", "con$P", "cont$"]);
    let shared = |target: &FnIdx| is_shared_wrapper(&program.functions[target.0 as usize].name);
    for function in &program.functions {
        for block in &function.blocks {
            let calls = block.stmts.iter().filter_map(|stmt| match stmt {
                Stmt::Let {
                    rhs: Rhs::Call(Call::Direct(target, _)),
                    ..
                } => Some(target),
                _ => None,
            });
            let tail = match &block.term {
                Term::TailCall(Call::Direct(target, _)) => Some(target),
                _ => None,
            };
            for target in calls.chain(tail) {
                assert!(
                    !shared(target),
                    "`{}` calls a shared wrapper",
                    function.name
                );
            }
        }
    }
}

#[test]
fn no_boxed_wrapper_is_made() {
    // 値として使うトップレベルの関数も型どおりの ABI のまま名前で参照し、`apply` が実行時に Repr を合わせる。一様な
    // 包み `f$boxed` は作らない (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「box の挿入」)
    let text = format!(
        "{HIDE}inc : Int -> <Panic> Int\ninc n = n + 1\n\n{APPLY_TO}main : Unit -> <IO, Panic> Unit\nmain () =\n  let Box f = hide 0 inc\n  println (show (apply_to f 2))"
    );
    let program = eml_test_support::lir_until(&text, eml_lir::Pass::Boxing);
    assert!(
        program
            .functions
            .iter()
            .all(|function| !function.name.ends_with("$boxed")),
        "{}",
        eml_lir::pretty(&program)
    );
    let shown = eml_lir::pretty(&program);
    insta::assert_snapshot!(function(&shown, "inc"), @r#"
    fn inc(n.0: int) -> int {
      let t.1: int = extern "Prelude.Num Int.+"(n.0, 1)
      return t.1
    }
    "#);
    assert!(shown.contains("{1=inc}"), "{shown}");
}

#[test]
fn fields_are_rebound_at_the_head_of_the_case_target_and_after_an_unpack() {
    let text = "data Option a =\n  | None\n  | Some a\n\nget : Option Int -> <Panic> Int\nget o = match o with\n  | Some v -> v + 1\n  | None -> 0\n\nsum : (Int, Int) -> <Panic> Int\nsum p =\n  let (a, b) = p\n  a + b\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box get = hide 0 get\n  let Box sum = hide 0 sum\n  println (show (get (Some 1) + sum (1, 2)))";
    let text = format!("{HIDE}{text}");
    let shown = lir_text(&text, eml_lir::Pass::Boxing);
    insta::assert_snapshot!(function(&shown, "get"), @"
    fn get(o.0: tobj) -> int {
      switch o.0 Option { #0 -> b1, #1(v.3: tobj) -> b2 }
    b1:
      return 0
    b2:
      let v.1: int = unbox v.3
      let t.2: int = extern \"Prelude.Num Int.+\"(v.1, 1)
      return t.2
    }
    ");
    insta::assert_snapshot!(function(&shown, "sum"), @"
    fn sum(p.0: obj) -> int {
      unpack p.0 (,) #0(a.4: tobj, b.5: tobj)
      let a.1: int = unbox a.4
      let b.2: int = unbox b.5
      let t.3: int = extern \"Prelude.Num Int.+\"(a.1, b.2)
      return t.3
    }
    ");
}

#[test]
fn int_constants_are_boxed_and_other_constants_pass_as_they_are() {
    // 型変数のフィールドは `tobj` なので、`int` の定数は `box` を通り、`unit`、タグ、関数の値はそのまま入る
    let text = "data Four a b c d =\n  | Four a b c d\n\ncount : Four a b c d -> Int\ncount _ = 0\n\nid : a -> a\nid x = x\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box count = hide 0 count\n  println (show (count (Four 5 () True id)))";
    let text = format!("{HIDE}{text}");
    insta::assert_snapshot!(function(&lir_text(&text, eml_lir::Pass::Boxing), "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: obj = call "hide@#29c654b2da42db65"(0)
      unpack t.1 Box #0(count.2: tobj)
      let b.7: tobj = box 5
      let d.3: obj = con Four #0(b.7, (), #1, &"id@[_]")
      let t.4: int = apply count.2(d.3)
      let t.5: obj = extern "Prelude.Show Int.show"(t.4)
      let t.6: unit = extern Prelude.println(t.5)
      return t.6
    }
    "#);
}

#[test]
fn a_value_unboxed_by_the_pass_is_passed_on_without_a_new_box() {
    // 継続の関数は再開の値と結果が一様である。入口で `unbox` した再開の値を返すときは、`box` を作らずに受けた `tobj`
    // をそのまま返す。`1` は新しく `box` する
    let text = format!(
        "effect Ask where\n  ask : Unit -> Int\n\n{HIDE}pick : Int -> <Ask, Panic> Int\npick n =\n  let x = ask ()\n  if x > n then x else n\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box body = hide 0 (fn () -> pick 1)\n  let r =\n    handle body () with\n      | ctl ask () k -> k 2\n  println (show r)"
    );
    insta::assert_snapshot!(function(&lir_text(&text, eml_lir::Pass::Boxing), "main$lambda0$k0"), @r#"
    internal yields role cont fn main$lambda0$k0(t.2: tobj) -> tobj {
      let t.0: int = unbox t.2
      let t.1: enum = extern "Prelude.Ord Int.>"(t.0, 1)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let b.3: tobj = box 1
      return b.3
    b2:
      return t.2
    }
    "#);
}

#[test]
fn applies_of_a_typed_function_value_convert_nothing() {
    // 型どおりの場所の `apply` は引数と結果を型どおりの Repr で渡すので、`apply` の結果をもう一度 `apply` に渡すときも
    // 変換しない (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「`apply` の ABI」)
    let text = format!(
        "{HIDE}twice_plus : (Int -> Int) -> Int -> <Panic> Int\ntwice_plus f x = f (f x) + 1\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box f = hide 0 (fn n -> n)\n  let Box twice_plus = hide 0 twice_plus\n  println (show (twice_plus f 1))"
    );
    insta::assert_snapshot!(function(&lir_text(&text, eml_lir::Pass::Boxing), "twice_plus"), @r#"
    fn twice_plus(f.0: tobj, x.1: int) -> int {
      let t.2: int = apply f.0(x.1)
      let t.3: int = apply f.0(t.2)
      let t.4: int = extern "Prelude.Num Int.+"(t.3, 1)
      return t.4
    }
    "#);
}

#[test]
fn a_unit_value_passes_to_tobj_without_an_instruction() {
    let text = format!(
        "{HIDE}wrap : Unit -> Box Unit\nwrap u = Box u\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box wrap = hide 0 wrap\n  let _ = wrap ()\n  println \"done\""
    );
    insta::assert_snapshot!(function(&lir_text(&text, eml_lir::Pass::Boxing), "wrap"), @"
    fn wrap(u.0: unit) -> obj {
      let d.1: obj = con Box #0(u.0)
      return d.1
    }
    ");
}

#[test]
fn a_monomorphic_int_loop_has_no_box() {
    let text = "loop : Int -> Int -> <Panic> Int\nloop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)\n\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (loop 3 0))";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Boxing), "loop"), @"
    fn loop(n.0: int, acc.1: int) -> int {
      let t.2: enum = extern \"Prelude.Eq Int.==\"(n.0, 0)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: int = extern \"Prelude.Num Int.-\"(n.0, 1)
      let t.4: int = extern \"Prelude.Num Int.+\"(acc.1, 1)
      let t.5: int = call loop(t.3, t.4)
      return t.5
    b2:
      return acc.1
    }
    ");
}

#[test]
fn a_function_that_fails_with_a_never_operation_keeps_its_scalar_ret() {
    // `never` の操作の `clause` は戻らないので、結果を受け直さない
    let text = format!(
        "{HIDE}effect Fail where\n  never fail : String -> a\n\ncheck_positive : Int -> <Fail> Int\ncheck_positive n =\n  if n > 0 then n else fail \"not positive\"\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box check = hide 0 check_positive\n  let r = handle check 1 with\n    | ctl fail message -> 0\n  println (show r)"
    );
    let text = text.as_str();
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Boxing), "check_positive"), @r#"
    fn check_positive(n.0: int) -> int {
      let t.1: enum = extern "Prelude.Ord Int.>"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "not positive"
      let ev.4: obj = prim ev_lookup[Fail]()
      let c.5: tobj = prim ev_clause[Fail.fail](ev.4)
      let t.3: int = clause Fail.fail c.5(ev.4, s.2)
      return t.3
    b2:
      return n.0
    }
    "#);
    // 縮約は、`never` の操作の `clause` をどの `ret` の関数でも末尾呼び出しにする
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Contract), "check_positive"), @r#"
    fn check_positive(n.0: int) -> int {
      let t.1: enum = extern "Prelude.Ord Int.>"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "not positive"
      let ev.4: obj = prim ev_lookup[Fail]()
      let c.5: tobj = prim ev_clause[Fail.fail](ev.4)
      tail clause Fail.fail c.5(ev.4, s.2)
    b2:
      return n.0
    }
    "#);
}

#[test]
fn a_value_lambda_that_fails_ends_in_a_tail_never_clause() {
    // 値として使うラムダでも、`never` の操作の `clause` の束縛は変換せずに返す。束縛の使いには制御が届かないから
    // である。縮約は、その `return` を末尾呼び出しにする
    let text = format!(
        "{HIDE}effect Fail where\n  never fail : String -> a\n\napply_to : (Int -> <e> Int) -> Int -> <e> Int\napply_to f x = f x\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box f = hide 0 (fn n -> if n > 0 then n else fail \"neg\")\n  let r = handle apply_to f 1 with\n    | ctl fail message -> 0\n  println (show r)"
    );
    insta::assert_snapshot!(function(&lir_text(&text, eml_lir::Pass::Boxing), "main$lambda0"), @r#"
    internal fn main$lambda0(n.0: int) -> int {
      let t.1: enum = extern "Prelude.Ord Int.>"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "neg"
      let ev.4: obj = prim ev_lookup[Fail]()
      let c.5: tobj = prim ev_clause[Fail.fail](ev.4)
      let t.3: int = clause Fail.fail c.5(ev.4, s.2)
      return t.3
    b2:
      return n.0
    }
    "#);
    insta::assert_snapshot!(function(&lir_text(&text, eml_lir::Pass::Contract), "main$lambda0"), @r#"
    internal fn main$lambda0(n.0: int) -> int {
      let t.1: enum = extern "Prelude.Ord Int.>"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "neg"
      let ev.4: obj = prim ev_lookup[Fail]()
      let c.5: tobj = prim ev_clause[Fail.fail](ev.4)
      tail clause Fail.fail c.5(ev.4, s.2)
    b2:
      return n.0
    }
    "#);
}

#[test]
fn a_chain_of_tail_calls_keeps_its_typed_rets() {
    // 関数の `ret` は型と役割だけで決まり、パスが上げない。`apply_to` は末尾の位置の `apply` でも `int` を返し、それを
    // 末尾の位置で呼ぶ `via` も `int` を返す。`through` は、呼び出しと `return` の間に使われない純粋な `let` があっても
    // 末尾呼び出しになる。`apply_to` は自分を呼ぶので `via` に展開されず、`via` と `through` はデータを通して呼ぶので
    // `main` に展開されない (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「関数の ABI」)
    let text = format!(
        "{HIDE}apply_to : (Int -> Int) -> Int -> <Panic> Int\napply_to f x = if x > 9 then apply_to f (x - 1) else f x\n\nvia : Int -> <Panic> Int\nvia x =\n  let Box f = hide 0 (fn n -> n)\n  apply_to f x\n\nthrough : (Int -> Int) -> Int -> Int\nthrough f x =\n  let r = f x\n  let s = \"unused\"\n  r\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box g = hide 0 (fn n -> n)\n  let Box via = hide 0 via\n  let Box through = hide 0 through\n  println (show (via 1 + through g 2))"
    );
    let shown = lir_text(&text, eml_lir::Pass::Contract);
    insta::assert_snapshot!(function(&shown, "apply_to"), @r#"
    fn apply_to(f.0: tobj poly, x.1: int) -> int {
      let t.2: enum = extern "Prelude.Ord Int.>"(x.1, 9)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      tail apply f.0(x.1)
    b2:
      let t.3: int = extern "Prelude.Num Int.-"(x.1, 1)
      tail call apply_to(f.0, t.3)
    }
    "#);
    insta::assert_snapshot!(function(&shown, "via"), @r#"
    fn via(x.0: int) -> int {
      let t.1: obj = call "hide@[Int -> Int]{1=via$lambda0}"(0)
      unpack t.1 Box #0(f.2: tobj)
      tail call apply_to(f.2, x.0)
    }
    "#);
    insta::assert_snapshot!(function(&shown, "through"), @"
    fn through(f.0: tobj poly, x.1: int) -> int {
      tail apply f.0(x.1)
    }
    ");
    insta::assert_snapshot!(function(&shown, "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: obj = call "hide@[Int -> Int]{1=main$lambda0}"(0)
      unpack t.1 Box #0(g.2: tobj)
      let t.3: obj = call "hide@[Int -> <Panic> Int]{1=via}"(0)
      unpack t.3 Box #0(via.4: tobj)
      let t.5: obj = call "hide@[(Int -> Int) -> Int -> Int]{1=through}"(0)
      unpack t.5 Box #0(through.6: tobj)
      let t.7: int = apply via.4(1: int)
      let t.8: int = apply through.6(g.2, 2: int)
      let t.9: int = extern "Prelude.Num Int.+"(t.7, t.8)
      let t.10: obj = extern "Prelude.Show Int.show"(t.9)
      let t.11: unit = extern Prelude.println(t.10)
      return t.11
    }
    "#);
}

#[test]
fn a_body_ending_in_a_typed_call_makes_it_a_tail_call() {
    // handle の本体は役割で結果が一様な `tobj` になり、`Int` を返す `answer` を末尾の位置で呼ぶ。box の挿入は結果を
    // 変換せず、縮約が末尾呼び出しにする。結果の変換は VM の変換のフレームが行う
    // (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「box の挿入」)
    let text = format!(
        "effect Ask where\n  ask : Unit -> Int\n\n{HIDE}answer : Int -> <Ask, Panic> Int\nanswer n =\n  if n > 0 then answer (n - 1)\n  else\n    let Box q = hide 0 ask\n    q () + 1\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let r = handle answer 0 with\n    | ctl ask () k -> k 1\n  println (show r)"
    );
    insta::assert_snapshot!(function(&lir_text(&text, eml_lir::Pass::Contract), "main$handle0"), @"
    internal yields role body fn main$handle0(p.0: unit) -> tobj {
      tail call answer(0)
    }
    ");
}

#[test]
fn the_result_of_exit_is_not_compared_with_positions() {
    // `exit` は戻らないので、`int` の束縛がタプルのフィールド (`tobj` の位置) に流れても `box` は入らない (docs/spec/lir.md の「戻らない extern」)
    let text = format!(
        "{HIDE}f : Int -> <IO> (Int, Int)\nf n = if n == 0 then (exit 3, n) else (n, n)\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box f = hide 0 f\n  println (show (f 1))"
    );
    insta::assert_snapshot!(function(&lir_text(&text, eml_lir::Pass::Boxing), "f"), @r#"
    fn f(n.0: int) -> obj {
      let t.1: enum = extern "Prelude.Eq Int.=="(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let n.5: tobj = box n.0
      let n.6: tobj = box n.0
      let d.4: obj = con (,) #0(n.5, n.6)
      return d.4
    b2:
      let t.2: int = extern Prelude.exit(3)
      let n.7: tobj = box n.0
      let d.3: obj = con (,) #0(t.2, n.7)
      return d.3
    }
    "#);
}

/// 関数の ABI は名前で決まり、ほかの定義の使い方によらない
/// (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「関数の ABI」)。値としても使う
/// ローカル関数 `d$let$g` は、`d` をどの深さの呼び出しから展開するプログラムでも、同じ LIR になる。
#[test]
fn a_local_function_used_as_a_value_has_one_abi_in_every_program() {
    const D: &str = "d : Int -> <Panic> Int\nd x =\n  let g y = y + 1\n  let fs = [g]\n  g x + List.length fs\n\n";
    let direct = format!("{D}main : Unit -> <IO, Panic> Unit\nmain () = println (display (d 5))");
    let mut chain = String::from(D);
    chain.push_str("x1 : Int -> <Panic> Int\nx1 n = d n\n\n");
    for i in 2..=6 {
        chain.push_str(&format!(
            "x{i} : Int -> <Panic> Int\nx{i} n = x{} n\n\n",
            i - 1
        ));
    }
    chain.push_str("main : Unit -> <IO, Panic> Unit\nmain () = println (display (x6 5))");
    let g = |text: &str| function(&lir_text(text, eml_lir::Pass::Perceus), "d$let$g");
    assert_eq!(g(&direct), g(&chain));
}
