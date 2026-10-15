//! コード生成の形を逆アセンブラの出力で固定する (docs/spec/vm.md の「命令とフレーム」)。

use crate::common::{disasm, disasm_unverified};

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
    fn main (arity 0, frame 3)
         2  load_raw s1, 40
         3  load_raw s2, 2
         4  call s0 = add(s1, s2)
         5  extern s0 = Prelude.Show Int.show(s0)
         6  extern s0 = Prelude.println(s0)
         7  ret s0
    fn add (arity 2, frame 2)
         8  int_add s0, s0, s1
         9  ret s0
    ");
}

#[test]
fn values_that_do_not_overlap_share_a_slot() {
    insta::assert_snapshot!(disasm("\
fn main() -> unit {
  let a.0: int = extern \"Prelude.+\"(1, 2)
  let b.1: int = extern \"Prelude.+\"(a.0, 3)
  let c.2: int = extern \"Prelude.+\"(b.1, 4)
  let s.3: obj = extern \"Prelude.Show Int.show\"(c.2)
  let u.4: unit = extern Prelude.println(s.3)
  return u.4
}
"), @"
    fn main (arity 0, frame 3)
         2  load_raw s1, 1
         3  load_raw s2, 2
         4  int_add s0, s1, s2
         5  load_raw s1, 3
         6  int_add s0, s0, s1
         7  load_raw s1, 4
         8  int_add s0, s0, s1
         9  extern s0 = Prelude.Show Int.show(s0)
        10  extern s0 = Prelude.println(s0)
        11  ret s0
    ");
}

/// 使われない呼び出しの結果も、その後で生きている値とは別のスロットを取る。同じスロットだと、`Ret` が結果を書いて
/// 生きている値を上書きする (docs/spec/vm.md の「スロットの割り当て」)。
#[test]
fn a_dead_call_result_takes_a_slot_of_its_own() {
    insta::assert_snapshot!(disasm_unverified("\
fn main() -> unit {
  let k.0: int = extern \"Prelude.+\"(1, 2)
  let r.1: int = call one() save [k.0]
  let s.2: obj = extern \"Prelude.Show Int.show\"(k.0)
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
fn one() -> int {
  return 1
}
"), @"
    fn main (arity 0, frame 4)
         2  load_raw s2, 1
         3  load_raw s3, 2
         4  int_add s0, s2, s3
         5  call s1 = one()
         6  extern s0 = Prelude.Show Int.show(s0)
         7  extern s0 = Prelude.println(s0)
         8  ret s0
    fn one (arity 0, frame 1)
         9  load_raw s0, 1
        10  ret s0
    ");
}

/// 実引数とブロック引数の色が入れ替わると、代入が輪になり、作業用のスロットを通る。
#[test]
fn swapped_block_arguments_go_through_the_scratch_slot() {
    insta::assert_snapshot!(disasm("\
fn main() -> unit {
  let x.0: int = extern \"Prelude.+\"(1, 0)
  let y.1: int = extern \"Prelude.+\"(2, 0)
  jump b1(y.1, x.0)
b1(p.2: int, q.3: int):
  let d.4: int = extern \"Prelude.-\"(p.2, q.3)
  let s.5: obj = extern \"Prelude.Show Int.show\"(d.4)
  let u.6: unit = extern Prelude.println(s.5)
  return u.6
}
"), @"
    fn main (arity 0, frame 4)
         2  load_raw s2, 1
         3  load_raw s3, 0
         4  int_add s0, s2, s3
         5  load_raw s2, 2
         6  load_raw s3, 0
         7  int_add s1, s2, s3
         8  move s2, s1
         9  move s1, s0
        10  move s0, s2
        11  int_sub s0, s0, s1
        12  extern s0 = Prelude.Show Int.show(s0)
        13  extern s0 = Prelude.println(s0)
        14  ret s0
    ");
}
