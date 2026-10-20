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
  let c.2: int = extern \"Prelude.Num Int.+\"(a.0, b.1)
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
  let a.0: int = extern \"Prelude.Num Int.+\"(1, 2)
  let b.1: int = extern \"Prelude.Num Int.+\"(a.0, 3)
  let c.2: int = extern \"Prelude.Num Int.+\"(b.1, 4)
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
  let k.0: int = extern \"Prelude.Num Int.+\"(1, 2)
  let r.1: int = call one()
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
  let x.0: int = extern \"Prelude.Num Int.+\"(1, 0)
  let y.1: int = extern \"Prelude.Num Int.+\"(2, 0)
  jump b1(y.1, x.0)
b1(p.2: int, q.3: int):
  let d.4: int = extern \"Prelude.Num Int.-\"(p.2, q.3)
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

/// フィールドのない `#N` の行き先にだけ入るブロックでは、scrutinee は即値なので `decref` を出さない
/// (docs/spec/vm.md の「RC」)。`#1` の行き先の `decref` は残る。
#[test]
fn a_field_less_case_drops_the_decref_of_its_immediate_scrutinee() {
    insta::assert_snapshot!(disasm("\
layout Option { None, Some(tobj) }
fn main() -> unit {
  tail call show(#0)
}
fn show(d.0: tobj) -> unit {
  switch d.0 Option { #0 -> b1, #1(x.1: obj) -> b2 }
b1:
  decref d.0
  let s.2: obj = const \"none\"
  let o.3: unit = extern Prelude.println(s.2)
  return o.3
b2:
  decref d.0
  return ()
}
"), @"
    fn main (arity 0, frame 1)
         2  load_imm s0, 0
         3  tail_call show(s0)
    fn show (arity 1, frame 3)
         4  switch_tobj s0 { #0 -> 5 [], #1 -> 8 [] }
         5  load_const s0, c0
         6  extern s0 = Prelude.println(s0)
         7  ret s0
         8  decref s0
         9  load_imm s2, 0
        10  ret s2
    ");
}

#[test]
fn a_release_keeps_the_named_fields() {
    insta::assert_snapshot!(disasm("\
layout Pair { Pair(tobj, int) }
fn main() -> unit {
  let s.0: obj = const \"kept\"
  let p.1: obj = con Pair #0(s.0, 7)
  unpack p.1 Pair #0(a.2: obj, n.3: int)
  release p.1 Pair #0(a.2, _)
  let u.4: unit = extern Prelude.println(a.2)
  return u.4
}
"), @"
    fn main (arity 0, frame 4)
         2  load_const s0, c0
         3  load_raw s3, 7
         4  con s0 = #0 (1 scanned, 1 raw) [@0 -> s0, raw @1 -> s3]
         5  unpack s0 #0 [@0 -> s1]
         6  release s0 #0 [keep @0]
         7  extern s0 = Prelude.println(s1)
         8  ret s0
    ");
}

/// case のフィールドを読む間、scrutinee のスロットはどのフィールドのスロットとも違う。
#[test]
fn a_switch_case_binds_fields_apart_from_its_scrutinee() {
    insta::assert_snapshot!(disasm("\
layout Pair { Pair(obj, obj) }
fn main() -> unit {
  let a.0: obj = const \"a\"
  let b.1: obj = const \"b\"
  let p.2: obj = con Pair #0(a.0, b.1)
  tail call first(p.2)
}
fn first(p.0: obj) -> unit {
  switch p.0 Pair { #0(x.1: obj, y.2: obj) -> b1 }
b1:
  release p.0 Pair #0(x.1, y.2)
  decref y.2
  let u.3: unit = extern Prelude.println(x.1)
  return u.3
}
"), @"
    fn main (arity 0, frame 2)
         2  load_const s0, c0
         3  load_const s1, c1
         4  con s0 = #0 (2 scanned, 0 raw) [@0 -> s0, @1 -> s1]
         5  tail_call first(s0)
    fn first (arity 1, frame 3)
         6  switch_obj s0 { #0 -> 7 [@0 -> s1, @1 -> s2] }
         7  release s0 #0 [keep @0, keep @1]
         8  decref s2
         9  extern s0 = Prelude.println(s1)
        10  ret s0
    ");
}

#[test]
fn an_unpack_binds_fields_apart_from_its_scrutinee() {
    insta::assert_snapshot!(disasm("\
layout Pair { Pair(obj, obj) }
fn main() -> unit {
  let a.0: obj = const \"a\"
  let b.1: obj = const \"b\"
  let p.2: obj = con Pair #0(a.0, b.1)
  unpack p.2 Pair #0(x.3: obj, y.4: obj)
  release p.2 Pair #0(x.3, y.4)
  decref y.4
  let u.5: unit = extern Prelude.println(x.3)
  return u.5
}
"), @"
    fn main (arity 0, frame 3)
         2  load_const s0, c0
         3  load_const s1, c1
         4  con s0 = #0 (2 scanned, 0 raw) [@0 -> s0, @1 -> s1]
         5  unpack s0 #0 [@0 -> s1, @1 -> s2]
         6  release s0 #0 [keep @0, keep @1]
         7  decref s2
         8  extern s0 = Prelude.println(s1)
         9  ret s0
    ");
}

/// ブロック引数への代入は、変数の移動を先に並べ、定数の読み込みを後に置く。`x.0` が飛ぶ文で死ぬので、定数の
/// 書き込み先が `x.0` のスロットになってもよい。
#[test]
fn a_jump_loads_constants_after_moving_variables() {
    insta::assert_snapshot!(disasm("\
fn main() -> unit {
  let x.0: int = extern \"Prelude.Num Int.+\"(1, 2)
  jump b1(5, x.0)
b1(a.1: int, b.2: int):
  let c.3: int = extern \"Prelude.Num Int.+\"(a.1, b.2)
  let s.4: obj = extern \"Prelude.Show Int.show\"(c.3)
  let u.5: unit = extern Prelude.println(s.4)
  return u.5
}
"), @"
    fn main (arity 0, frame 4)
         2  load_raw s2, 1
         3  load_raw s3, 2
         4  int_add s0, s2, s3
         5  move s1, s0
         6  load_raw s0, 5
         7  int_add s0, s0, s1
         8  extern s0 = Prelude.Show Int.show(s0)
         9  extern s0 = Prelude.println(s0)
        10  ret s0
    ");
}

/// 死んでいる scrutinee (借りたフィールド `f`) のスロットも、case のフィールドのスロットとは違う。干渉の辺がないと、
/// 先に色を取る `a.2` が `f.1` のスロットを取り、switch が `a` を書いた後で `b` を読めなくなる (docs/spec/vm.md の「スロットの割り当て」)。
#[test]
fn a_dead_switch_scrutinee_does_not_share_a_slot_with_the_case_fields() {
    insta::assert_snapshot!(disasm("\
layout Inner { Inner(obj, obj) }
layout Outer { Outer(obj) }
fn main() -> unit {
  let a.0: obj = const \"a\"
  let b.1: obj = const \"b\"
  let i.2: obj = con Inner #0(a.0, b.1)
  let o.3: obj = con Outer #0(i.2)
  tail call go(o.3)
}
fn go(o.0: obj) -> unit {
  unpack o.0 Outer #0(f.1: obj)
  switch f.1 Inner { #0(a.2: obj, b.3: obj) -> b1 }
b1:
  dup a.2
  dup b.3
  decref o.0
  let u.4: unit = extern Prelude.println(a.2)
  let v.5: unit = extern Prelude.println(b.3)
  return v.5
}
"), @"
    fn main (arity 0, frame 2)
         2  load_const s0, c0
         3  load_const s1, c1
         4  con s0 = #0 (2 scanned, 0 raw) [@0 -> s0, @1 -> s1]
         5  con s0 = #0 (1 scanned, 0 raw) [@0 -> s0]
         6  tail_call go(s0)
    fn go (arity 1, frame 4)
         7  unpack s0 #0 [@0 -> s1]
         8  switch_obj s1 { #0 -> 9 [@0 -> s2, @1 -> s3] }
         9  dup s2
        10  dup s3
        11  decref s0
        12  extern s0 = Prelude.println(s2) unwind [s3]
        13  extern s0 = Prelude.println(s3)
        14  ret s0
    ");
}

/// `i32` に収まらない同じ値の定数は、定数の表に1つだけ置く。
#[test]
fn a_wide_constant_is_stored_once() {
    insta::assert_snapshot!(disasm("\
fn main() -> unit {
  let a.0: int = extern \"Prelude.Num Int.+\"(5000000000, 1)
  let b.1: int = extern \"Prelude.Num Int.+\"(a.0, 5000000000)
  let s.2: obj = extern \"Prelude.Show Int.show\"(b.1)
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
"), @"
    fn main (arity 0, frame 3)
         2  load_const s1, c0
         3  load_raw s2, 1
         4  int_add s0, s1, s2
         5  load_const s1, c0
         6  int_add s0, s0, s1
         7  extern s0 = Prelude.Show Int.show(s0)
         8  extern s0 = Prelude.println(s0)
         9  ret s0
    ");
}

/// case のない `switch` は、値を比べずに `default` へ進むので、`goto` になる。
#[test]
fn a_switch_without_cases_is_a_goto() {
    insta::assert_snapshot!(disasm("\
layout Bool { False, True }
fn main() -> unit {
  let n.0: int = extern \"Prelude.Num Int.+\"(1, 2)
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 3)
  switch d.1 Bool { #0 -> b2, #1 -> b1 }
b1:
  switch n.0 { _ -> b3 }
b2:
  return ()
b3:
  let s.2: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
"), @"
    fn main (arity 0, frame 4)
         2  load_raw s2, 1
         3  load_raw s3, 2
         4  int_add s0, s2, s3
         5  load_raw s2, 3
         6  int_eq s1, s0, s2
         7  switch_enum s1 { #0 -> 9 [], #1 -> 8 [] }
         8  goto 11
         9  load_imm s2, 0
        10  ret s2
        11  extern s0 = Prelude.Show Int.show(s0)
        12  extern s0 = Prelude.println(s0)
        13  ret s0
    ");
}

/// `enum` の scrutinee の `switch` は、生の値をタグとして読む `switch_enum` になる。フィールドのない case だけなので、
/// どの行き先にも束縛はない。
#[test]
fn a_switch_on_an_enum_is_switch_enum() {
    insta::assert_snapshot!(disasm("\
layout Bool { False, True }
fn main() -> unit {
  tail call pick(3)
}
fn pick(n.0: int) -> unit {
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 3)
  switch d.1 Bool { #0 -> b1, #1 -> b2 }
b1:
  return ()
b2:
  let s.2: obj = const \"three\"
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
"), @"
    fn main (arity 0, frame 1)
         2  load_raw s0, 3
         3  tail_call pick(s0)
    fn pick (arity 1, frame 2)
         4  load_raw s1, 3
         5  int_eq s0, s0, s1
         6  switch_enum s0 { #0 -> 7 [], #1 -> 9 [] }
         7  load_imm s1, 0
         8  ret s1
         9  load_const s0, c0
        10  extern s0 = Prelude.println(s0)
        11  ret s0
    ");
}

/// `Int` の `switch` は、キーと行き先を表に並べる `switch_int` になる。`i32` に収まらないキーも表に入る。
#[test]
fn a_switch_on_an_int_is_switch_int() {
    insta::assert_snapshot!(disasm("\
fn main() -> unit {
  tail call pick(2)
}
fn pick(n.0: int) -> unit {
  switch n.0 { 1 -> b1, 5000000000 -> b2, _ -> b3 }
b1:
  let s.1: obj = const \"one\"
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
b2:
  let s.3: obj = const \"big\"
  let u.4: unit = extern Prelude.println(s.3)
  return u.4
b3:
  return ()
}
"), @"
    fn main (arity 0, frame 1)
         2  load_raw s0, 2
         3  tail_call pick(s0)
    fn pick (arity 1, frame 2)
         4  switch_int s0 { 1 -> 5, 5000000000 -> 8, _ -> 11 }
         5  load_const s0, c0
         6  extern s0 = Prelude.println(s0)
         7  ret s0
         8  load_const s0, c1
         9  extern s0 = Prelude.println(s0)
        10  ret s0
        11  load_imm s1, 0
        12  ret s1
    ");
}

/// `String` の `switch` は、キーを定数の表の文字列で指す `switch_string` になる。
#[test]
fn a_switch_on_a_string_is_switch_string() {
    insta::assert_snapshot!(disasm("\
fn main() -> unit {
  let s.0: obj = const \"a\"
  let n.1: int = call pick(s.0)
  return ()
}
fn pick(s.0: obj) -> int {
  switch s.0 { \"a\" -> b1, \"b\" -> b2, _ -> b3 }
b1:
  decref s.0
  return 1
b2:
  decref s.0
  return 2
b3:
  decref s.0
  return 3
}
"), @"
    fn main (arity 0, frame 2)
         2  load_const s0, c0
         3  call s0 = pick(s0)
         4  load_imm s1, 0
         5  ret s1
    fn pick (arity 1, frame 2)
         6  switch_string s0 { 0 -> 7, 1 -> 10, _ -> 13 }
         7  decref s0
         8  load_raw s1, 1
         9  ret s1
        10  decref s0
        11  load_raw s1, 2
        12  ret s1
        13  decref s0
        14  load_raw s1, 3
        15  ret s1
    ");
}

/// 巻き戻しの地点の命令の後に、表の行を書く。行は命令の後で所有しているスロット (2つ以上の参照は `*n`) と、着地の
/// 印の evidence のスロットである。手放すものも着地もない行は書かない
/// (docs/spec/vm.md の「巻き戻しの表」)。
#[test]
fn an_unwind_point_shows_its_owned_slots_and_landing() {
    insta::assert_snapshot!(disasm("\
fn main() -> unit {
  return ()
}
yields fn land(b.0: tobj, ev.1: obj, s.2: obj) -> unit {
  dup s.2
  let x.3: tobj = yields apply b.0((): unit) prompt ev.1
  decref x.3
  decref ev.1
  let t.4: obj = extern \"Prelude.++\"(s.2, s.2)
  let u.5: unit = extern Prelude.println(t.4)
  return u.5
}
"), @"
    fn main (arity 0, frame 1)
         2  load_imm s0, 0
         3  ret s0
    fn land (arity 3, frame 4)
         4  dup s2
         5  load_imm s3, 0
         6  apply s0 = s0(s3) unwind [s1, s2*2] land s1
         7  decref s0
         8  decref s1
         9  extern s0 = Prelude.++(s2, s2)
        10  extern s0 = Prelude.println(s0)
        11  ret s0
    ");
}

/// reuse の命令。token を定義する `release` は token のスロットを、token を受ける `con` は token のスロット、
/// `in place`、書かないフィールド (`skip`) を書く。巻き戻しの表の行は、生きている token のスロットを `token` の後に
/// 並べる (docs/spec/vm.md の「reuse」)。
#[test]
fn reuse_instructions_disassemble() {
    insta::assert_snapshot!(disasm("\
layout P { P(obj, int) }
layout Q { Q(obj, int) }
fn main() -> unit {
  let a.0: obj = const \"a\"
  let p.1: obj = con P #0(a.0, 7)
  tail call go(p.1)
}
fn go(p.0: obj) -> unit {
  unpack p.0 P #0(x.1: obj, n.2: int)
  let r.3: token = release p.0 P #0(x.1, _)
  let s.4: obj = const \"!\"
  let u.5: unit = extern Prelude.println(s.4)
  switch n.2 { 0 -> b1, 1 -> b2, _ -> b3 }
b1:
  let q.6: obj = con P #0(=x.1, n.2) reuse r.3 in place
  tail call sink(q.6)
b2:
  let q.7: obj = con Q #0(x.1, 8) reuse r.3
  tail call sink(q.7)
b3:
  drop_token r.3
  decref x.1
  return u.5
}
fn sink(q.0: obj) -> unit {
  decref q.0
  return ()
}
"), @"
    fn main (arity 0, frame 2)
         2  load_const s0, c0
         3  load_raw s1, 7
         4  con s0 = #0 (1 scanned, 1 raw) [@0 -> s0, raw @1 -> s1]
         5  tail_call go(s0)
    fn go (arity 1, frame 5)
         6  unpack s0 #0 [@0 -> s1, raw @1 -> s2]
         7  release s0 #0 [keep @0] token s0
         8  load_const s3, c1
         9  extern s3 = Prelude.println(s3) unwind [s1] tokens [s0]
        10  switch_int s2 { 0 -> 11, 1 -> 13, _ -> 16 }
        11  con s0 = #0 (1 scanned, 1 raw) [skip @0 -> s1, raw @1 -> s2] reuse s0 in place
        12  tail_call sink(s0)
        13  load_raw s4, 8
        14  con s0 = #0 (1 scanned, 1 raw) [@0 -> s1, raw @1 -> s4] reuse s0
        15  tail_call sink(s0)
        16  drop_token s0
        17  decref s1
        18  ret s3
    fn sink (arity 1, frame 2)
        19  decref s0
        20  load_imm s1, 0
        21  ret s1
    ");
}

/// `float` の箱。定数の `box` は不死の `Float` の定数を写し、変数の `box` は `box_float` で確保し、`unbox` は束縛する
/// 変数の Repr で `unbox_float` を選ぶ。生の定数はビット列を `int` の定数と同じ形で読む。
#[test]
fn floats_are_boxed_and_unboxed_by_their_own_instructions() {
    insta::assert_snapshot!(disasm("\
layout P { P(float, tobj) }
fn main() -> unit {
  let a.0: tobj = box 0.0
  let b.1: tobj = box -0.0
  let f.2: float = unbox b.1
  decref b.1
  let c.3: tobj = box f.2
  let p.4: obj = con P #0(-0.0, c.3)
  decref p.4
  decref a.0
  return ()
}
"), @"
    fn main (arity 0, frame 4)
         2  box_const s0, c0
         3  box_const s1, c1
         4  unbox_float s2, s1
         5  decref s1
         6  box_float s1, s2
         7  load_const s3, c2
         8  con s1 = #0 (1 scanned, 1 raw) [raw @1 -> s3, @0 -> s1]
         9  decref s1
        10  decref s0
        11  load_imm s3, 0
        12  ret s3
    ");
}

/// `Float` の算術と比較の演算子は専用の命令になり、`compare` はほかの extern と同じ `extern` の命令になる
/// (docs/spec/vm.md の「基本操作と extern」)。
#[test]
fn float_arithmetic_and_comparisons_get_their_own_instructions() {
    insta::assert_snapshot!(disasm("\
fn main(a.0: float, b.1: float) -> enum {
  let c.2: float = extern \"Prelude.Num Float.+\"(a.0, b.1)
  let d.3: float = extern \"Prelude.Div Float.%\"(c.2, 1.5)
  let e.4: float = extern \"Prelude.Num Float.negate\"(d.3)
  let f.5: enum = extern \"Prelude.Ord Float.<\"(e.4, a.0)
  let g.6: enum = extern \"Prelude.Ord Float.compare\"(a.0, b.1)
  let h.7: enum = extern \"Prelude.Eq Bool.==\"(f.5, g.6)
  return h.7
}
"), @"
    fn main (arity 2, frame 4)
         2  float_add s2, s0, s1
         3  load_const s3, c0
         4  float_rem s2, s2, s3
         5  float_neg s2, s2
         6  float_lt s2, s2, s0
         7  extern s0 = Prelude.Ord Float.compare(s0, s1)
         8  bool_eq s0, s2, s0
         9  ret s0
    ");
}

/// 配列の `length` / `get` / `read` / `set` / `write` は専用の命令になり、汎用の `extern` を通らない
/// (docs/spec/vm.md の「基本操作と extern」)。
#[test]
fn array_rows_use_dedicated_ops() {
    let text = disasm_unverified(
        "\
fn main(a.0: obj, m.1: obj) -> unit {
  let n.2: int = extern Std.Array.length(a.0)
  let x.3: tobj = extern Std.Array.get(a.0, n.2)
  let k.4: int = extern Std.MutArray.length(m.1)
  let y.5: tobj = extern Std.MutArray.read(m.1, k.4)
  let b.6: obj = extern Std.Array.set(a.0, n.2, x.3)
  let c.7: obj = extern Std.MutArray.write(m.1, k.4, y.5)
  return ()
}
",
    );
    for op in ["array_length", "array_get", "array_set"] {
        assert!(text.contains(op), "{op} is missing:\n{text}");
    }
    assert!(
        !text.contains("extern"),
        "an array row stayed generic:\n{text}"
    );
}
