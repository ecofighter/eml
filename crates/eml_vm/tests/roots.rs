//! セッションの根を読み書きする基本操作 (docs/spec/lir.md の「基本操作」)。根に置いた値は実行の終わりに VM が
//! 手放すので、検査モードのリークの検査も通る。

use eml_rt::{Fault, HeapError};
use eml_vm::{RuntimeError, VmFault};

use crate::common::{disasm, run_lir};

/// 根に置いた文字列と整数を読み直して表示する。整数は `tobj` に box して置き、文字列はそのまま `obj` で読む。
#[test]
fn a_value_set_in_a_root_is_read_back() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"kept\"
  let u.1: unit = prim root_set[0](s.0)
  let b.2: tobj = box 42
  let u.3: unit = prim root_set[7](b.2)
  let t.4: obj = prim root_get[0]()
  let v.5: unit = extern Prelude.println(t.4)
  let c.6: tobj = prim root_get[7]()
  let n.7: int = unbox c.6
  decref c.6
  let r.8: obj = extern \"Prelude.Show Int.show\"(n.7)
  let w.9: unit = extern Prelude.println(r.8)
  return w.9
}
";
    assert_eq!(run_lir(text), ("kept\n42\n".to_string(), Ok(())));
}

/// 空の根は、どちらのモードでも読めない。
#[test]
fn reading_an_empty_root_is_an_error() {
    let text = "\
fn main() -> unit {
  let t.0: tobj = prim root_get[3]()
  decref t.0
  return ()
}
";
    assert_eq!(
        run_lir(text).1,
        Err(RuntimeError::Fault {
            fault: VmFault::Extern(Fault::Heap(HeapError::RootMisuse)),
            function: "main".to_string(),
            at: None,
        })
    );
}

#[test]
fn root_primitives_name_their_root() {
    insta::assert_snapshot!(disasm("\
fn main() -> unit {
  let b.0: tobj = box 1
  let u.1: unit = prim root_set[2](b.0)
  let t.2: tobj = prim root_get[2]()
  decref t.2
  return u.1
}
"), @"
    fn main (arity 0, frame 2)
        14  box_const s0, c0
        15  root_set s0, r2, s0
        16  root_get s1, r2
        17  decref s1
        18  ret s0
    ");
}
