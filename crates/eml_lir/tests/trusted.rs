//! `Trusted`の2つの構築子と、それが作る表 (docs/spec/lir.mdの「エンジンの約束」、docs/spec/lir.mdの「巻き戻しの地点」)。

use eml_lir::unwind::{UnwindPoint, Unwinds};
use eml_lir::{BlockId, Program, Trusted, VarId, parse};

fn read(text: &str) -> Program {
    parse(text).unwrap_or_else(|error| panic!("{error}"))
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

fn unwinds_of<'a>(trusted: &'a Trusted, name: &str) -> &'a Unwinds {
    let index = trusted
        .functions
        .iter()
        .position(|function| function.name == name)
        .unwrap_or_else(|| panic!("no `{name}`"));
    &trusted.unwinds()[index]
}

/// `Trusted::new`はどのビルドでも確かめ、verifierが拒むものをその誤りで断り、verifierが受け入れるプログラムの表を
/// 持つ。
#[test]
fn new_refuses_what_the_verifier_refuses() {
    let refused = "\
fn main() -> unit {
  let s.0: obj = const \"kept\"
  return ()
}
";
    // SAFETY: プログラムは実行しない。テストは誤りだけを読む
    let error = unsafe { Trusted::new(read(refused)) }.unwrap_err();
    assert_eq!(error.function, "main");
    assert_eq!(
        error.to_string(),
        "`s.0` is still owned at the end of the function in `main`"
    );
    // 関数ごとの走査だけでは当てないプログラム全体の規則。`clause`の印の関数のABIである
    let program_wide = "\
effect E { op(unit) -> unit }
fn main() -> unit {
  return ()
}
internal clause E.op fn g(e.0: cev, x.1: float) -> unit {
  decref e.0
  return ()
}
";
    // SAFETY: プログラムは実行しない。テストは誤りだけを読む
    let error = unsafe { Trusted::new(read(program_wide)) }.unwrap_err();
    assert_eq!(error.function, "g", "{error}");
    let accepted = "\
fn main(x.0: obj) -> obj {
  let z.1: obj = extern Prelude.exit(1)
  decref x.0
  return z.1
}
";
    // SAFETY: プログラムは実行しない。テストは表だけを読む
    let trusted = unsafe { Trusted::new(read(accepted)) }.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        trusted.unwinds(),
        [Unwinds {
            entry: vec![(VarId(0), 1)],
            points: vec![point(0, 0, &[(0, 1)], None)],
        }]
    );
}

/// 確かめない`Trusted`では、所有の段の走査が拒む関数は、空のエントリの行と、地点 (呼び出し、失敗しうるextern、
/// 印のある`apply`) ごとに、何も手放さず着地の印を持つ行を持つ。同じプログラムで走査を通る関数は、本当の表を持つ。
#[test]
fn an_unverified_program_has_a_row_at_every_point() {
    let text = "\
effect Land { land(unit) -> tobj }
fn entry$main() -> unit {
  return ()
}
fn g(x.0: obj) -> obj {
  let z.1: obj = extern Prelude.exit(1)
  decref x.0
  return z.1
}
yields fn f(b.0: tobj, w.5: evv, k.6: obj) -> tobj {
  let u.4: unit = extern Prelude.println(k.6)
  let ev.1: ev lin = evidence Land((), w.5) [&land$entry]
  let x.2: tobj = yields apply b.0(()) prompt ev.1
  let r.3: tobj = call prompt$(ev.1, x.2)
  let s.7: obj = const \"kept\"
  return r.3
}
yields fn prompt$(ev.0: ev lin, x.1: tobj poly) -> tobj poly {
  let u.2: unit = drop ev.0
  return x.1
}
internal clause Land.land fn land$entry(e.0: cev, u.1: unit) -> tobj {
  decref e.0
  return ()
}
";
    let program = read(text);
    assert_eq!(
        eml_lir::verify(&program).map_err(|error| error.to_string()),
        Err("`s.7` is still owned at the end of the function in `f`".to_string())
    );
    // SAFETY: プログラムは実行しない。テストは表だけを読む
    let trusted = unsafe { Trusted::unverified(program) };
    assert_eq!(
        unwinds_of(&trusted, "f"),
        &Unwinds {
            entry: Vec::new(),
            points: vec![
                point(0, 0, &[], None),
                point(0, 2, &[], Some(1)),
                point(0, 3, &[], None),
            ],
        }
    );
    assert_eq!(
        unwinds_of(&trusted, "g"),
        &Unwinds {
            entry: vec![(VarId(0), 1)],
            points: vec![point(0, 0, &[(0, 1)], None)],
        }
    );
}
