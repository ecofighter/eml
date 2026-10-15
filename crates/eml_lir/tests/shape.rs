//! データの物理的な形 (docs/spec/object-model.md の「ペイロード」) と、文の単位の生存の公開の API。

use std::collections::BTreeSet;

use eml_lir::liveness::step_back;
use eml_lir::{LayoutCtor, Repr, Shape, VarId};

fn ctor(fields: &[Repr]) -> LayoutCtor {
    LayoutCtor {
        name: "C".to_string(),
        fields: fields.to_vec(),
    }
}

#[test]
fn scanned_fields_come_first_in_declared_order() {
    let shape = ctor(&[Repr::Int, Repr::TObj, Repr::Unit, Repr::Obj, Repr::Enum]).shape();
    assert_eq!(
        shape,
        Shape {
            positions: vec![2, 0, 3, 1, 4],
            scanned: 2,
            raw: 3,
        }
    );
}

#[test]
fn a_cons_cell_is_the_element_then_the_rest() {
    let shape = ctor(&[Repr::TObj, Repr::Obj]).shape();
    assert_eq!(shape.positions, [0, 1]);
    assert_eq!((shape.scanned, shape.raw), (2, 0));
}

#[test]
fn raw_fields_alone_keep_their_order() {
    let shape = ctor(&[Repr::Int, Repr::Enum]).shape();
    assert_eq!(shape.positions, [0, 1]);
    assert_eq!((shape.scanned, shape.raw), (0, 2));
}

#[test]
fn stepping_back_over_a_statement_removes_its_definitions_and_adds_its_uses() {
    let program = eml_lir::parse(
        "fn main() -> unit {\n  let s.0: obj = const \"a\"\n  let u.1: unit = extern Prelude.println(s.0)\n  return u.1\n}\n",
    )
    .unwrap();
    let block = &program.functions[0].blocks[0];
    let mut live = BTreeSet::from([VarId(1)]);
    step_back(&block.stmts[1], &mut live);
    assert_eq!(live, BTreeSet::from([VarId(0)]));
}
