//! コード生成の形を逆アセンブラの出力で固定する (docs/spec/vm.md の「命令とフレーム」)。

use crate::common::disasm;

#[test]
fn a_call_passes_its_arguments_by_slot() {
    insta::assert_snapshot!(disasm("\
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
"), @"
    fn main (arity 0, frame 5)
         2  load_raw s3, 40
         3  load_raw s4, 2
         4  call s0 = add(s3, s4)
         5  extern s1 = Prelude.Show Int.show(s0)
         6  extern s2 = Prelude.println(s1)
         7  ret s2
    fn add (arity 2, frame 3)
         8  int_add s2, s0, s1
         9  ret s2
    ");
}
