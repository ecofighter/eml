//! 生存解析 (docs/spec/core-ir.md の「パス」)。

use eml_lir::liveness::live_in;
use eml_lir::{VarId, parse};

#[test]
fn live_in_holds_every_variable_used_later() {
    // RC の対象でない変数も入る。`saved` が使うためである。jump の引数と case のフィールドは行き先の入口で生きている
    let program = parse(
        "\
layout Prelude.Bool { False, True }
layout Option { None, Some(tobj) }
fn f(x.0: int, o.1: tobj) -> int {
  let c.2: enum = extern \"Prelude.Ord Int.<\"(x.0, 10)
  switch c.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  jump b4(x.0)
b2:
  switch o.1 Option { #0 -> b3, #1(y.3: int) -> b5 }
b3:
  jump b4(1)
b4(t.4: int):
  return t.4
b5:
  return y.3
}
",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let sets = live_in(&program.functions[0]);
    let expected: [&[u32]; 6] = [&[0, 1], &[0], &[1], &[], &[4], &[3]];
    assert_eq!(
        sets,
        expected.map(|set| set.iter().copied().map(VarId).collect::<Vec<_>>())
    );
}
