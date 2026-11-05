//! C の関数の呼び出しの変換 (docs/spec/core-ir.md の「組を返す extern」と「translate の配置」)。

use crate::common::{core_text, function, lir_text, session_core_text};
use eml_core_ir::Pass;
use eml_extern::{CFnId, CHandleType, CLibrary, CLink, ExternRef, Os, Purity};

/// ハンドルを作る C の関数、配列とハンドルを借りて書き込む C の関数、純粋な C の関数。
const SOLVER: &str = "\
import MutUnboxedArray

link on linux \"solver\"
link on macos framework \"Solver\"

extern \"C\" data Solver drop \"solver_free\"

extern \"C\" symbol \"solver_new\" open : Int as int -> <IO, Panic> Solver
extern \"C\" fill : MutUnboxedArray Float -> Solver -> Int -> <IO> (Unit, MutUnboxedArray Float, Solver)
extern \"C\" pure cos : Float -> Float
";

/// `SOLVER` の後に置く `main`。3つの C の関数をどれも1回ずつ呼ぶ。
const MAIN: &str = "
main : Unit -> <IO, Panic> Unit
main () =
  let s = open 3
  let a : MutUnboxedArray Float = MutUnboxedArray.empty ()
  let ((), a, s) = fill a s 1
  drop a
  drop s
  println (show (cos 0.0))
";

#[test]
fn a_c_call_is_an_extern_statement() {
    // 引数のそろった C の呼び出しは、std の extern と同じく `extern` の文になり、C の表の正式な名前を書く。`lin` の
    // 引数がないので、結果は組でなく `R` のままである
    let text = "\
link on linux \"m\"

extern \"C\" pure cos : Float -> Float

main : Unit -> <IO> Unit
main () = println (show (cos 0.0))
";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    effect Prelude.IO { never io_failed(enum, obj) -> never }
    effect Prelude.Panic { never panicked(obj) -> never }
    cfn "Main.cos" symbol "cos" pure link [linux "m"] (f64) -> f64
    fn main(p.0: unit) -> unit {
      let t.1: float = extern Main.cos(0.0)
      let t.2: obj = extern "Prelude.Show Float.show"(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    "#);
}

#[test]
fn lin_arguments_come_back_in_a_tuple() {
    // `lin` の引数が2つあるので、結果と2つの引数の3つ組を `con` で作る。C の行は配列とハンドルを借りるだけなので、
    // 2つを消費するのは `con` である
    let text = format!("{SOLVER}{MAIN}");
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let t.1: obj lin = extern Main.open(3)
      let t.2: obj lin = extern Std.MutUnboxedArray.empty(())
      let t.3: unit = extern Main.fill(t.2, t.1, 1)
      let d.4: (,,){unit, obj lin, obj lin} lin = con (,,) #0(t.3, t.2, t.1)
      unpack d.4 (,,) #0(x.5: unit, a.6: obj lin, s.7: obj lin)
      let t.8: unit = drop a.6
      let t.9: unit = drop s.7
      let t.10: float = extern Main.cos(0.0)
      let t.11: obj = extern "Prelude.Show Float.show"(t.10)
      let t.12: unit = extern Prelude.println(t.11)
      return t.12
    }
    "#);
    // 組が借りた引数を受け取る形のまま、Perceus と reuse の後の verifier まで通る
    let lowered = lir_text(&text, eml_lir::Pass::Reuse);
    assert!(lowered.contains("extern Main.fill("), "{lowered}");
}

#[test]
fn a_partial_c_call_goes_through_a_wrapper() {
    // 部分適用は std の extern と同じく `$externN` の閉包にし、包む関数の本体が組を作る
    let text = format!(
        "{SOLVER}
step : MutUnboxedArray Float -> Solver -> <IO> (Unit, MutUnboxedArray Float, Solver)
step a s =
  let g = fill a
  g s 2

main : Unit -> <IO, Panic> Unit
main () =
  let s = open 3
  let a : MutUnboxedArray Float = MutUnboxedArray.empty ()
  let ((), a, s) = step a s
  drop a
  drop s
"
    );
    let shown = core_text(&text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "step") + &function(&shown, "step$extern0"), @r#"
    fn step(a.0: obj lin, s.1: obj lin) -> (,,){unit, obj lin, obj lin} lin {
      let c.2: tobj lin = closure step$extern0(a.0)
      let t.3: (,,){unit, obj lin, obj lin} lin = apply c.2(s.1, 2)
      return t.3
    }
    internal uniform from 0 fn step$extern0(p.0: obj lin, p.1: obj lin, p.2: int) -> (,,){unit, obj lin, obj lin} lin {
      let t.3: unit = extern Main.fill(p.0, p.1, p.2)
      let d.4: (,,){unit, obj lin, obj lin} lin = con (,,) #0(t.3, p.0, p.1)
      return d.4
    }
    "#);
}

#[test]
fn the_c_table_lists_used_functions_in_first_use_order() {
    // 宣言の順でなく、変換が最初に使った順に並ぶ。使わない C の関数は表に入らない
    let text = "\
extern \"C\" pure first : Int -> Int
extern \"C\" pure second : Int -> Int
extern \"C\" pure unused : Int -> Int

main : Unit -> <IO> Unit
main () =
  let b = second 1
  let a = first b
  let c = second a
  println (show c)
";
    let program = eml_test_support::core_until(text, Pass::Translate);
    let names: Vec<&str> = program
        .c_functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(names, ["Main.second", "Main.first"]);
    assert!(program.c_handles.is_empty());
    // REPL のセッションの C の関数は、ほかの item と同じく中段の名前を持つ。入力 2 が定義し直した `first` は
    // `first#2` である (docs/spec/core-ir.md の「REPL の入力」)
    let shown = session_core_text(
        &[
            "extern \"C\" pure first : Int -> Int",
            "extern \"C\" pure first : Int -> Int",
            "first 1",
        ],
        Pass::Translate,
    );
    let tables: Vec<&str> = shown
        .lines()
        .filter(|line| line.starts_with("cfn "))
        .collect();
    assert_eq!(
        tables,
        [r#"cfn "first#2" symbol "first" pure link [] (i64) -> i64"#]
    );
    // 中段の名前は `.` を含まないので、std の行を先に引く `ExternRef::from_name` でも std の行と取り違えない
    assert!(
        eml_extern::Extern::ALL
            .iter()
            .all(|ext| ext.row().name.contains('.'))
    );
}

#[test]
fn a_handle_type_carries_its_drop_and_links() {
    // ハンドルの型は、型を宣言したモジュールの `link` を持ち、C の関数は自分を宣言したモジュールの `link` を持つ
    let native = "\
link on linux \"native\"

pub extern \"C\" data Handle drop \"handle_free\"
";
    let entry = "\
import Native

link on windows \"main\"

extern \"C\" make : Unit -> <IO, Panic> Native.Handle

main : Unit -> <IO, Panic> Unit
main () = drop (make ())
";
    let program = eml_test_support::core_until(
        eml_test_support::with_modules(entry, &[("Native.em", native)]),
        Pass::Translate,
    );
    assert_eq!(
        program.c_handles,
        [CHandleType {
            name: "Native.Handle".to_string(),
            dtor: Some("handle_free".to_string()),
            links: vec![CLink {
                os: Os::Linux,
                library: CLibrary::Named("native".to_string()),
            }],
        }]
    );
    assert_eq!(
        program.c_functions[0].links,
        [CLink {
            os: Os::Windows,
            library: CLibrary::Named("main".to_string()),
        }]
    );
}

#[test]
fn the_c_row_of_a_translated_call() {
    // translate が作った行は、宣言の属性と型検査が確かめた形から決まる (docs/spec/lir.md の「extern の行」)
    let text = format!("{SOLVER}{MAIN}");
    let program = eml_test_support::core_until(&text, Pass::Translate);
    let names: Vec<&str> = program
        .c_functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(names, ["Main.open", "Main.fill", "Main.cos"]);
    let row = |index: u32| program.row(ExternRef::C(CFnId(index)));
    let open = row(0);
    assert_eq!(
        (open.purity, open.faults, open.writes),
        (Purity::Effectful, true, false)
    );
    assert_eq!(program.c_functions[0].symbol, "solver_new");
    let fill = row(1);
    assert_eq!(
        (fill.purity, fill.faults, fill.writes),
        (Purity::Effectful, false, true)
    );
    assert_eq!(fill.borrowed, [true, true, false]);
    assert_eq!(fill.with_args, [0, 1]);
    let cos = row(2);
    assert_eq!(
        (cos.purity, cos.faults, cos.writes),
        (Purity::Pure, false, false)
    );
    assert!(cos.removable());
}
