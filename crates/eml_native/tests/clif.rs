//! CLIFの固定。生成したコードの速い道と冷たい道の形を、関数1つのコンパイルの前のCLIFで固定する
//! (docs/spec/native.md の「巻き戻しの地点」)。
#![cfg(not(miri))]
use std::ffi::c_void;

use eml_cabi::Linked;

use crate::common::native::twin;
use crate::common::native::{clif, clif_linked, machine, program};

#[test]
fn an_arith_row() {
    // 割り算の地点は`s.0`を持つ。0除算と桁あふれの冷たいブロックは、それぞれ失敗を置いて上げ、1つの後始末へ進む。後始末は
    // `s.0`を手放してから仮の値を返す
    let text = "\
fn f(s.0: obj, a.1: int, b.2: int) -> obj {
  let q.3: int = extern \"Prelude.Div Int./\"(a.1, b.2) @\"main.em\":1:1
  return s.0
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64, i64, i64) -> i64 tail {
        sig0 = (i64, i64, i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64, i64, i64) tail
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = %eml_rt_fault_arith sig3
        fn4 = colocated %eml$raise sig4

    block0(v0: i64, v1: i64, v2: i64, v3: i64):
        v4 = load.i64 notrap aligned v0+40
        v5 = load.i32 notrap aligned v4
        v6 = iconst.i32 0
        v7 = icmp ne v5, v6  ; v6 = 0
        v8 = get_stack_pointer.i64 
        v9 = load.i64 notrap aligned v0+48
        v10 = icmp ult v8, v9
        v11 = bor v7, v10
        brif v11, block1, block3

    block1 cold:
        v12 = func_addr.i64 fn0
        v13 = iconst.i64 0
        v14 = call fn1(v0, v12, v13)  ; v13 = 0
        v15 = iconst.i64 0
        v16 = icmp eq v14, v15  ; v15 = 0
        brif v16, block3, block2

    block2 cold:
        call fn2(v0, v1)
        v17 = iconst.i64 1
        return v17  ; v17 = 1

    block3:
        v18 = iconst.i64 0
        v19 = icmp.i64 eq v3, v18  ; v18 = 0
        brif v19, block4, block5

    block5:
        v20 = iconst.i64 -9223372036854775808
        v21 = iconst.i64 -1
        v22 = icmp.i64 eq v2, v20  ; v20 = -9223372036854775808
        v23 = icmp.i64 eq v3, v21  ; v21 = -1
        v24 = band v22, v23
        brif v24, block6, block7

    block6 cold:
        v25 = iconst.i64 1
        call fn3(v0, v25)  ; v25 = 1
        v26 = func_addr.i64 fn0
        v27 = iconst.i64 0
        v28 = iconst.i64 1
        call fn4(v0, v26, v27, v28)  ; v27 = 0, v28 = 1
        jump block8

    block4 cold:
        v29 = iconst.i64 0
        call fn3(v0, v29)  ; v29 = 0
        v30 = func_addr.i64 fn0
        v31 = iconst.i64 0
        v32 = iconst.i64 1
        call fn4(v0, v30, v31, v32)  ; v31 = 0, v32 = 1
        jump block8

    block8 cold:
        call fn2(v0, v1)
        v33 = iconst.i64 1
        return v33  ; v33 = 1

    block7:
        v34 = sdiv.i64 v2, v3
        return v1
    }
    ");
}

#[test]
fn a_faulting_row() {
    // 1つ目の`println`の地点は`t.1`を持つので、後始末はそれを手放してから仮の値を返す
    let text = "\
fn f(s.0: obj, t.1: obj) -> unit {
  let u.2: unit = extern Prelude.println(s.0) @\"main.em\":1:1
  let v.3: unit = extern Prelude.println(t.1)
  return v.3
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64, i64) -> i64 tail {
        sig0 = (i64, i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) -> i64 c
        sig4 = (i64, i64, i64, i64) tail
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = %eml_x_Println sig3
        fn4 = colocated %eml$raise sig4

    block0(v0: i64, v1: i64, v2: i64):
        v3 = load.i64 notrap aligned v0+40
        v4 = load.i32 notrap aligned v3
        v5 = iconst.i32 0
        v6 = icmp ne v4, v5  ; v5 = 0
        v7 = get_stack_pointer.i64 
        v8 = load.i64 notrap aligned v0+48
        v9 = icmp ult v7, v8
        v10 = bor v6, v9
        brif v10, block1, block3

    block1 cold:
        v11 = func_addr.i64 fn0
        v12 = iconst.i64 0
        v13 = call fn1(v0, v11, v12)  ; v12 = 0
        v14 = iconst.i64 0
        v15 = icmp eq v13, v14  ; v14 = 0
        brif v15, block3, block2

    block2 cold:
        call fn2(v0, v1)
        call fn2(v0, v2)
        v16 = iconst.i64 1
        return v16  ; v16 = 1

    block3:
        v17 = call fn3(v0, v1)
        v18 = load.i64 notrap aligned v0+32
        brif v18, block4, block6

    block4 cold:
        v19 = func_addr.i64 fn0
        v20 = iconst.i64 0
        v21 = iconst.i64 2
        call fn4(v0, v19, v20, v21)  ; v20 = 0, v21 = 2
        jump block5

    block5 cold:
        call fn2(v0, v2)
        v22 = iconst.i64 1
        return v22  ; v22 = 1

    block6:
        v23 = call fn3(v0, v2)
        v24 = load.i64 notrap aligned v0+32
        brif v24, block7, block9

    block7 cold:
        v25 = func_addr.i64 fn0
        v26 = iconst.i64 1
        v27 = iconst.i64 2
        call fn4(v0, v25, v26, v27)  ; v26 = 1, v27 = 2
        jump block8

    block8 cold:
        v28 = iconst.i64 1
        return v28  ; v28 = 1

    block9:
        return v23
    }
    ");
}

#[test]
fn a_row_that_does_not_fault() {
    let text = "\
fn f(n.0: int) -> obj {
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  return s.1
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64) -> i64 tail {
        sig0 = (i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) -> i64 c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_x_IntShow sig2

    block0(v0: i64, v1: i64):
        v2 = load.i64 notrap aligned v0+40
        v3 = load.i32 notrap aligned v2
        v4 = iconst.i32 0
        v5 = icmp ne v3, v4  ; v4 = 0
        v6 = get_stack_pointer.i64 
        v7 = load.i64 notrap aligned v0+48
        v8 = icmp ult v6, v7
        v9 = bor v5, v8
        brif v9, block1, block3

    block1 cold:
        v10 = func_addr.i64 fn0
        v11 = iconst.i64 0
        v12 = call fn1(v0, v10, v11)  ; v11 = 0
        v13 = iconst.i64 0
        v14 = icmp eq v12, v13  ; v13 = 0
        brif v14, block3, block2

    block2 cold:
        v15 = iconst.i64 1
        return v15  ; v15 = 1

    block3:
        v16 = call fn2(v0, v1)
        return v16
    }
    ");
}

#[test]
fn a_never_row() {
    // `exit`の後の続きは届かないブロックに下ろし、その変数には仮の値を置く
    let text = "\
fn f(s.0: obj) -> unit {
  let z.1: unit = extern Prelude.exit(3)
  let u.2: unit = extern Prelude.println(s.0)
  return z.1
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64) -> i64 tail {
        sig0 = (i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64) -> i64 c
        sig5 = (i64, i64, i64, i64) tail
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = %eml_x_Exit sig3
        fn4 = %eml_x_Println sig4
        fn5 = colocated %eml$raise sig5

    block0(v0: i64, v1: i64):
        v2 = load.i64 notrap aligned v0+40
        v3 = load.i32 notrap aligned v2
        v4 = iconst.i32 0
        v5 = icmp ne v3, v4  ; v4 = 0
        v6 = get_stack_pointer.i64 
        v7 = load.i64 notrap aligned v0+48
        v8 = icmp ult v6, v7
        v9 = bor v5, v8
        brif v9, block1, block3

    block1 cold:
        v10 = func_addr.i64 fn0
        v11 = iconst.i64 0
        v12 = call fn1(v0, v10, v11)  ; v11 = 0
        v13 = iconst.i64 0
        v14 = icmp eq v12, v13  ; v13 = 0
        brif v14, block3, block2

    block2 cold:
        call fn2(v0, v1)
        v15 = iconst.i64 1
        return v15  ; v15 = 1

    block3:
        v16 = iconst.i64 3
        call fn3(v0, v16)  ; v16 = 3
        jump block4

    block4 cold:
        call fn2(v0, v1)
        v17 = iconst.i64 1
        return v17  ; v17 = 1

    block5:
        v18 = iconst.i64 1
        v19 = call fn4(v0, v1)
        v20 = load.i64 notrap aligned v0+32
        brif v20, block6, block8

    block6 cold:
        v21 = func_addr.i64 fn0
        v22 = iconst.i64 0
        v23 = iconst.i64 2
        call fn5(v0, v21, v22, v23)  ; v22 = 0, v23 = 2
        jump block7

    block7 cold:
        v24 = iconst.i64 1
        return v24  ; v24 = 1

    block8:
        return v18  ; v18 = 1
    }
    ");
}

#[test]
fn a_call_and_its_unwind_check() {
    // 呼び出しの後で`abort`のバイトを確かめ、中断なら持っている`s.0`を手放して仮の値を返す。呼ぶ先はどれも
    // moduleの中の記号なので、`colocated`と出る
    let text = "\
fn g(n.0: int) -> int {
  return n.0
}
fn h(s.0: obj, n.1: int) -> unit {
  let u.2: unit = extern Prelude.println(s.0)
  return u.2
}
fn f(s.0: obj, n.1: int) -> unit {
  let m.2: int = call g(n.1)
  tail call h(s.0, m.2)
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64, i64) -> i64 tail {
        sig0 = (i64, i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) -> i64 tail
        sig4 = (i64, i64, i64) -> i64 tail
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = colocated %f:g sig3
        fn4 = colocated %f:h sig4

    block0(v0: i64, v1: i64, v2: i64):
        v3 = load.i64 notrap aligned v0+40
        v4 = load.i32 notrap aligned v3
        v5 = iconst.i32 0
        v6 = icmp ne v4, v5  ; v5 = 0
        v7 = get_stack_pointer.i64 
        v8 = load.i64 notrap aligned v0+48
        v9 = icmp ult v7, v8
        v10 = bor v6, v9
        brif v10, block1, block3

    block1 cold:
        v11 = func_addr.i64 fn0
        v12 = iconst.i64 0
        v13 = call fn1(v0, v11, v12)  ; v12 = 0
        v14 = iconst.i64 0
        v15 = icmp eq v13, v14  ; v14 = 0
        brif v15, block3, block2

    block2 cold:
        call fn2(v0, v1)
        v16 = iconst.i64 1
        return v16  ; v16 = 1

    block3:
        v17 = call fn3(v0, v2)
        v18 = load.i8 notrap aligned v0+24
        brif v18, block4, block5

    block4 cold:
        call fn2(v0, v1)
        v19 = iconst.i64 1
        return v19  ; v19 = 1

    block5:
        return_call fn4(v0, v1, v17)
    }
    ");
}

#[test]
fn the_entry_safepoint() {
    // 入口のブロックは止める語と`stack_limit`を読み、どちらかで冷たいサービスへ分かれる。サービスが続けさせなければ、
    // 入口で持つ`s.0`を手放してから仮の値を返す
    let text = "\
fn entry$main() -> unit {
  return ()
}
fn f(s.0: obj, x.1: float) -> (obj, float) {
  return (s.0, x.1)
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64, f64) -> i64, f64 tail {
        sig0 = (i64, i64, f64) -> i64, f64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2

    block0(v0: i64, v1: i64, v2: f64):
        v3 = load.i64 notrap aligned v0+40
        v4 = load.i32 notrap aligned v3
        v5 = iconst.i32 0
        v6 = icmp ne v4, v5  ; v5 = 0
        v7 = get_stack_pointer.i64 
        v8 = load.i64 notrap aligned v0+48
        v9 = icmp ult v7, v8
        v10 = bor v6, v9
        brif v10, block1, block3

    block1 cold:
        v11 = func_addr.i64 fn0
        v12 = iconst.i64 0
        v13 = call fn1(v0, v11, v12)  ; v12 = 0
        v14 = iconst.i64 0
        v15 = icmp eq v13, v14  ; v14 = 0
        brif v15, block3, block2

    block2 cold:
        call fn2(v0, v1)
        v16 = iconst.i64 1
        v17 = f64const 0.0
        return v16, v17  ; v16 = 1, v17 = 0.0

    block3:
        return v1, v2
    }
    ");
}

#[test]
fn inline_dup_and_decref() {
    // 速い道は、即値の確かめ (`tobj`の`y.1`だけで、`obj`の`x.0`にはない)、数のロード1つ、範囲の確かめ1つ、ストア1つで
    // ある。0の数 (不死) は何も呼ばず、ほかの数は遅い道のエントリを呼ぶ
    let text = "\
fn f(x.0: obj, borrowed y.1: tobj) -> tobj {
  decref x.0
  dup y.1
  return y.1
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64, i64) -> i64 tail {
        sig0 = (i64, i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64) c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = %eml_rt_decref_slow sig3
        fn4 = %eml_rt_dup_slow sig4

    block0(v0: i64, v1: i64, v2: i64):
        v3 = load.i64 notrap aligned v0+40
        v4 = load.i32 notrap aligned v3
        v5 = iconst.i32 0
        v6 = icmp ne v4, v5  ; v5 = 0
        v7 = get_stack_pointer.i64 
        v8 = load.i64 notrap aligned v0+48
        v9 = icmp ult v7, v8
        v10 = bor v6, v9
        brif v10, block1, block3

    block1 cold:
        v11 = func_addr.i64 fn0
        v12 = iconst.i64 0
        v13 = call fn1(v0, v11, v12)  ; v12 = 0
        v14 = iconst.i64 0
        v15 = icmp eq v13, v14  ; v14 = 0
        brif v15, block3, block2

    block2 cold:
        call fn2(v0, v1)
        v16 = iconst.i64 1
        return v16  ; v16 = 1

    block3:
        v17 = load.i32 notrap aligned v1
        v18 = iconst.i32 -2
        v19 = iadd v17, v18  ; v18 = -2
        v20 = iconst.i32 0x7fff_fffd
        v21 = icmp ult v19, v20  ; v20 = 0x7fff_fffd
        brif v21, block4, block5

    block4:
        v22 = iconst.i32 -1
        v23 = iadd.i32 v17, v22  ; v22 = -1
        store notrap aligned v23, v1
        jump block7

    block5:
        brif.i32 v17, block6, block7

    block6 cold:
        call fn3(v0, v1)
        jump block7

    block7:
        v24 = iconst.i64 1
        v25 = band.i64 v2, v24  ; v24 = 1
        brif v25, block11, block12

    block12:
        v26 = load.i32 notrap aligned v2
        v27 = iconst.i32 -1
        v28 = iadd v26, v27  ; v27 = -1
        v29 = iconst.i32 0x7fff_fffe
        v30 = icmp ult v28, v29  ; v29 = 0x7fff_fffe
        brif v30, block8, block9

    block8:
        v31 = iconst.i32 1
        v32 = iadd.i32 v26, v31  ; v31 = 1
        store notrap aligned v32, v2
        jump block11

    block9:
        brif.i32 v26, block10, block11

    block10 cold:
        call fn4(v0, v2)
        jump block11

    block11:
        return v2
    }
    ");
}

#[test]
fn a_tag_switch() {
    // `tobj`のタグは、即値ならその値、物体ならヘッダのタグで、その後`Switch`で分かれる。caseの欄は行き先の先頭で、決まった
    // ずれのロード1つで読む
    let text = "\
layout Opt { None, Some(int, float) }
fn f(borrowed o.0: tobj) -> int {
  switch o.0 Opt { #0 -> b1, #1(n.1: int, x.2: float) -> b2 }
b1:
  return 0
b2:
  return n.1
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64) -> i64 tail {
        sig0 = (i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1

    block0(v0: i64, v1: i64):
        v2 = load.i64 notrap aligned v0+40
        v3 = load.i32 notrap aligned v2
        v4 = iconst.i32 0
        v5 = icmp ne v3, v4  ; v4 = 0
        v6 = get_stack_pointer.i64 
        v7 = load.i64 notrap aligned v0+48
        v8 = icmp ult v6, v7
        v9 = bor v5, v8
        brif v9, block1, block3

    block1 cold:
        v10 = func_addr.i64 fn0
        v11 = iconst.i64 0
        v12 = call fn1(v0, v10, v11)  ; v11 = 0
        v13 = iconst.i64 0
        v14 = icmp eq v12, v13  ; v13 = 0
        brif v14, block3, block2

    block2 cold:
        v15 = iconst.i64 0
        return v15  ; v15 = 0

    block3:
        v17 = iconst.i64 1
        v18 = band.i64 v1, v17  ; v17 = 1
        brif v18, block7, block8

    block7:
        v19 = iconst.i64 1
        v20 = sshr.i64 v1, v19  ; v19 = 1
        v21 = ireduce.i32 v20
        jump block9(v21)

    block8:
        v22 = load.i32 notrap aligned v1+4
        v23 = iconst.i32 0xffff
        v24 = band v22, v23  ; v23 = 0xffff
        jump block9(v24)

    block9(v16: i32):
        br_table v16, block6, [block4, block5]

    block6 cold:
        trap user1

    block4:
        v25 = iconst.i64 0
        return v25  ; v25 = 0

    block5:
        v26 = load.i64 notrap aligned v1+8
        v27 = load.f64 notrap aligned v1+16
        return v26
    }
    ");
}

#[test]
fn an_apply_site() {
    // 呼ぶ値のタグが引数の数と等しければ、コードの語を読んでそのエントリを間接に呼び、そうでなければ`eml$apply$1`を
    // 呼ぶ。2つの道は結果を持って合流し、そこで中断を確かめる。後始末は`apply`をまたいで持つ`s.0`を手放す
    let text = "\
fn f(s.0: obj, c.1: tobj, x.2: tobj) -> obj {
  let r.3: tobj = apply c.1(x.2)
  decref r.3
  return s.0
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64, i64, i64) -> i64 tail {
        sig0 = (i64, i64, i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64, i64) -> i64 tail
        sig4 = (i64, i64, i64) -> i64 tail
        sig5 = (i64, i64) c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = colocated %eml$apply$1 sig4
        fn4 = %eml_rt_decref_slow sig5

    block0(v0: i64, v1: i64, v2: i64, v3: i64):
        v4 = load.i64 notrap aligned v0+40
        v5 = load.i32 notrap aligned v4
        v6 = iconst.i32 0
        v7 = icmp ne v5, v6  ; v6 = 0
        v8 = get_stack_pointer.i64 
        v9 = load.i64 notrap aligned v0+48
        v10 = icmp ult v8, v9
        v11 = bor v7, v10
        brif v11, block1, block3

    block1 cold:
        v12 = func_addr.i64 fn0
        v13 = iconst.i64 0
        v14 = call fn1(v0, v12, v13)  ; v13 = 0
        v15 = iconst.i64 0
        v16 = icmp eq v14, v15  ; v15 = 0
        brif v16, block3, block2

    block2 cold:
        call fn2(v0, v1)
        call fn2(v0, v2)
        call fn2(v0, v3)
        v17 = iconst.i64 1
        return v17  ; v17 = 1

    block3:
        v18 = load.i32 notrap aligned v2+4
        v19 = iconst.i32 0xffff
        v20 = band v18, v19  ; v19 = 0xffff
        v21 = iconst.i32 1
        v22 = icmp eq v20, v21  ; v21 = 1
        brif v22, block4, block5

    block4:
        v24 = iconst.i32 16
        v25 = ushr.i32 v18, v24  ; v24 = 16
        v26 = iconst.i32 255
        v27 = band v25, v26  ; v26 = 255
        v28 = iconst.i32 255
        v29 = icmp eq v27, v28  ; v28 = 255
        v30 = iconst.i64 8
        v31 = iconst.i64 16
        v32 = select v29, v31, v30  ; v31 = 16, v30 = 8
        v33 = iadd.i64 v2, v32
        v34 = load.i64 notrap aligned v33
        v35 = iconst.i64 1
        v36 = sshr v34, v35  ; v35 = 1
        v37 = call_indirect sig3, v36(v0, v2, v3)
        jump block6(v37)

    block5:
        v38 = call fn3(v0, v2, v3)
        jump block6(v38)

    block6(v23: i64):
        v39 = load.i8 notrap aligned v0+24
        brif v39, block7, block8

    block7 cold:
        call fn2(v0, v1)
        v40 = iconst.i64 1
        return v40  ; v40 = 1

    block8:
        v41 = iconst.i64 1
        v42 = band.i64 v23, v41  ; v41 = 1
        brif v42, block12, block13

    block13:
        v43 = load.i32 notrap aligned v23
        v44 = iconst.i32 -2
        v45 = iadd v43, v44  ; v44 = -2
        v46 = iconst.i32 0x7fff_fffd
        v47 = icmp ult v45, v46  ; v46 = 0x7fff_fffd
        brif v47, block9, block10

    block9:
        v48 = iconst.i32 -1
        v49 = iadd.i32 v43, v48  ; v48 = -1
        store notrap aligned v49, v23
        jump block12

    block10:
        brif.i32 v43, block11, block12

    block11 cold:
        call fn4(v0, v23)
        jump block12

    block12:
        return v1
    }
    ");
}

#[test]
fn an_over_application() {
    // `eml$apply$2`の`n < m`の腕は、内の適用の後で呼び出しの地点と同じく`abort`のバイトを確かめ、その冷たい
    // ブロックが最後の引数を手放す。熱い側では`yielding.target`をロードし、Unitでなければtrapする。1つの`apply`の
    // うちyieldしうるのは最後の部分だけだからである
    let text = "\
fn f(x.0: tobj) -> tobj {
  return x.0
}
fn g(a.0: tobj, b.1: tobj) -> tobj {
  tail apply &f(a.0, b.1)
}
";
    insta::assert_snapshot!(clif(text, "eml$apply$2"), @"
    function %eml$apply$2(i64, i64, i64, i64) -> i64 tail {
        ss0 = explicit_slot 16, align = 8
        sig0 = (i64, i64, i64, i64) -> i64 tail
        sig1 = (i64, i64, i64, i64) -> i64 c
        sig2 = (i64, i64, i64, i64) -> i64 tail
        sig3 = (i64, i64, i64) -> i64 c
        sig4 = (i64, i64, i64) -> i64 tail
        sig5 = (i64, i64) c
        sig6 = (i64, i64, i64) -> i64 tail
        fn0 = %eml_nrt_pap sig1
        fn1 = colocated %eml$apply$2 sig2
        fn2 = %eml_nrt_safepoint sig3
        fn3 = colocated %eml$apply$1 sig4
        fn4 = %eml_rt_decref sig5
        fn5 = colocated %eml$apply$1 sig6

    block0(v0: i64, v1: i64, v2: i64, v3: i64):
        v4 = load.i32 notrap aligned v1+4
        v5 = iconst.i32 0xffff
        v6 = band v4, v5  ; v5 = 0xffff
        v7 = iconst.i32 2
        v8 = icmp eq v6, v7  ; v7 = 2
        brif v8, block1, block2

    block1:
        v9 = iconst.i32 16
        v10 = ushr.i32 v4, v9  ; v9 = 16
        v11 = iconst.i32 255
        v12 = band v10, v11  ; v11 = 255
        v13 = iconst.i32 255
        v14 = icmp eq v12, v13  ; v13 = 255
        v15 = iconst.i64 8
        v16 = iconst.i64 16
        v17 = select v14, v16, v15  ; v16 = 16, v15 = 8
        v18 = iadd.i64 v1, v17
        v19 = load.i64 notrap aligned v18
        v20 = iconst.i64 1
        v21 = sshr v19, v20  ; v20 = 1
        return_call_indirect sig0, v21(v0, v1, v2, v3)

    block2:
        v22 = iconst.i32 2
        v23 = icmp.i32 ugt v6, v22  ; v22 = 2
        brif v23, block3, block4

    block3:
        v24 = stack_addr.i64 ss0
        store.i64 notrap v2, v24
        v25 = stack_addr.i64 ss0+8
        store.i64 notrap v3, v25
        v26 = stack_addr.i64 ss0
        v27 = iconst.i64 2
        v28 = call fn0(v0, v1, v26, v27)  ; v27 = 2
        return v28

    block4:
        v29 = get_stack_pointer.i64 
        v30 = load.i64 notrap aligned v0+48
        v31 = icmp ult v29, v30
        brif v31, block5, block6

    block5 cold:
        v32 = func_addr.i64 fn1
        v33 = iconst.i64 0
        v34 = call fn2(v0, v32, v33)  ; v33 = 0
        v35 = iconst.i64 0
        v36 = icmp eq v34, v35  ; v35 = 0
        brif v36, block6, block7

    block7 cold:
        trap user1

    block6:
        v37 = call fn3(v0, v1, v2)
        v38 = load.i8 notrap aligned v0+24
        brif v38, block8, block9

    block8 cold:
        call fn4(v0, v3)
        v39 = iconst.i64 1
        return v39  ; v39 = 1

    block9:
        v40 = load.i64 notrap aligned v0
        v41 = iconst.i64 1
        v42 = icmp ne v40, v41  ; v41 = 1
        brif v42, block10, block11

    block10 cold:
        trap user1

    block11:
        return_call fn5(v0, v37, v3)
    }
    ");
}

extern "C" fn eml_test_c_pin(_n: i32, _text: *const u8) -> *mut c_void {
    std::ptr::null_mut()
}

extern "C" fn eml_test_c_pin_free(_handle: *mut c_void) {}

#[test]
fn a_c_row() {
    // 範囲の確かめが冷たいブロックへ分岐した後で、文字列の中身の位置を借り、`int32_t`の引数を狭めてCの関数を直接呼ぶ。
    // ハンドルの結果はdtorのアドレスと一緒に`eml_x_c_handle_new`に渡し、そのすぐ後でfaultの語を確かめる。冷たい
    // ブロックは1つの後始末へ進む
    let text = "\
chandle \"M.H\" drop \"eml_test_c_pin_free\" link []
cfn \"M.make\" symbol \"eml_test_c_pin\" link [] (i32, text) -> handle \"M.H\"
fn f(n.0: int, borrowed s.1: obj) -> obj lin {
  let h.2: obj lin = extern M.make(n.0, s.1) @\"main.em\":1:1
  return h.2
}
";
    let program = program(text);
    // SAFETY: どちらの関数も、宣言の署名を持つ`extern "C" fn`で、プロセスの間ずっとある。CLIFを作るだけで呼ばない
    let mut linker = unsafe {
        eml_clink::Linker::new(Vec::new()).with_symbols(&[
            ("eml_test_c_pin", eml_test_c_pin as *const ()),
            ("eml_test_c_pin_free", eml_test_c_pin_free as *const ()),
        ])
    };
    let linked = linker
        .link(&program.c_functions, &program.c_handles)
        .unwrap_or_else(|error| panic!("{error}"));
    insta::assert_snapshot!(clif_linked(text, "f", &linked), @"
    function %f:f(i64, i64, i64) -> i64 tail {
        gv0 = symbol colocated %s:M.make
        sig0 = (i64, i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64, i64, i64, i64) c
        sig3 = (i64, i64, i64, i64) tail
        sig4 = (i64, i64) -> i64 c
        sig5 = (i32, i64) -> i64 c
        sig6 = (i64) c
        sig7 = (i64, i64, i64, i64) -> i64 c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_x_c_narrow sig2
        fn3 = colocated %eml$raise sig3
        fn4 = %eml_rt_text_payload sig4
        fn5 = %eml_c$M.make sig5
        fn6 = %eml_cd$M.H sig6
        fn7 = %eml_x_c_handle_new sig7

    block0(v0: i64, v1: i64, v2: i64):
        v3 = load.i64 notrap aligned v0+40
        v4 = load.i32 notrap aligned v3
        v5 = iconst.i32 0
        v6 = icmp ne v4, v5  ; v5 = 0
        v7 = get_stack_pointer.i64 
        v8 = load.i64 notrap aligned v0+48
        v9 = icmp ult v7, v8
        v10 = bor v6, v9
        brif v10, block1, block3

    block1 cold:
        v11 = func_addr.i64 fn0
        v12 = iconst.i64 0
        v13 = call fn1(v0, v11, v12)  ; v12 = 0
        v14 = iconst.i64 0
        v15 = icmp eq v13, v14  ; v14 = 0
        brif v15, block3, block2

    block2 cold:
        v16 = iconst.i64 1
        return v16  ; v16 = 1

    block3:
        v17 = ireduce.i32 v1
        v18 = sextend.i64 v17
        v19 = icmp ne v18, v1
        brif v19, block5, block6

    block5 cold:
        v20 = symbol_value.i64 gv0
        v21 = load.i64 notrap aligned v20
        v22 = iconst.i64 1
        v23 = iconst.i64 1
        call fn2(v0, v21, v22, v23, v1)  ; v22 = 1, v23 = 1
        v24 = func_addr.i64 fn0
        v25 = iconst.i64 0
        v26 = iconst.i64 2
        call fn3(v0, v24, v25, v26)  ; v25 = 0, v26 = 2
        jump block4

    block4 cold:
        v27 = iconst.i64 1
        return v27  ; v27 = 1

    block6:
        v28 = call fn4(v0, v2)
        v29 = call fn5(v17, v28)
        v30 = symbol_value.i64 gv0
        v31 = load.i64 notrap aligned v30
        v32 = func_addr.i64 fn6
        v33 = call fn7(v0, v31, v32, v29)
        v34 = load.i64 notrap aligned v0+32
        brif v34, block7, block8

    block7 cold:
        v35 = func_addr.i64 fn0
        v36 = iconst.i64 0
        v37 = iconst.i64 2
        call fn3(v0, v35, v36, v37)  ; v36 = 0, v37 = 2
        jump block4

    block8:
        return v33
    }
    ");
}

#[test]
fn an_effect_lookup() {
    // 節の呼び出しは、エフェクトの番号をデータの記号`e:Counter`からロード1つで読んで`ev_lookup`のエントリに渡す。
    // エフェクトの番号を持つ即値はない (docs/spec/native.mdの「エフェクト」)。`ev_clause`のエントリが読んだクロージャの
    // コードを、evidenceと操作の引数で呼ぶ (docs/spec/native.mdの「関数の値」)
    let text = "\
effect Counter { get(unit) -> tobj }
fn f() -> tobj {
  let r.0: tobj = clause Counter.get(())
  return r.0
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64) -> i64 tail {
        gv0 = symbol colocated %e:Counter
        sig0 = (i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) -> i64 c
        sig3 = (i64, i64, i64) -> i64 c
        sig4 = (i64, i64, i64, i64) -> i64 tail
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_ev_lookup sig2
        fn3 = %eml_rt_ev_clause sig3

    block0(v0: i64):
        v1 = load.i64 notrap aligned v0+40
        v2 = load.i32 notrap aligned v1
        v3 = iconst.i32 0
        v4 = icmp ne v2, v3  ; v3 = 0
        v5 = get_stack_pointer.i64 
        v6 = load.i64 notrap aligned v0+48
        v7 = icmp ult v5, v6
        v8 = bor v4, v7
        brif v8, block1, block3

    block1 cold:
        v9 = func_addr.i64 fn0
        v10 = iconst.i64 0
        v11 = call fn1(v0, v9, v10)  ; v10 = 0
        v12 = iconst.i64 0
        v13 = icmp eq v11, v12  ; v12 = 0
        brif v13, block3, block2

    block2 cold:
        v14 = iconst.i64 1
        return v14  ; v14 = 1

    block3:
        v15 = symbol_value.i64 gv0
        v16 = load.i64 notrap aligned v15
        v17 = call fn2(v0, v16)
        v18 = iconst.i64 0
        v19 = call fn3(v0, v17, v18)  ; v18 = 0
        v20 = iconst.i64 1
        v21 = load.i32 notrap aligned v19+4
        v22 = iconst.i32 16
        v23 = ushr v21, v22  ; v22 = 16
        v24 = iconst.i32 255
        v25 = band v23, v24  ; v24 = 255
        v26 = iconst.i32 255
        v27 = icmp eq v25, v26  ; v26 = 255
        v28 = iconst.i64 8
        v29 = iconst.i64 16
        v30 = select v27, v29, v28  ; v29 = 16, v28 = 8
        v31 = iadd v19, v30
        v32 = load.i64 notrap aligned v31
        v33 = iconst.i64 1
        v34 = sshr v32, v33  ; v33 = 1
        v35 = call_indirect sig4, v34(v0, v19, v17, v20)  ; v20 = 1
        v36 = load.i8 notrap aligned v0+24
        brif v36, block4, block5

    block4 cold:
        v37 = iconst.i64 1
        return v37  ; v37 = 1

    block5:
        return v35
    }
    ");
}

#[test]
fn a_yield_point_call() {
    // yieldしうる関数の呼び出しの後は、`abort`のバイトだけを確かめる。オフセット24の8ビットのロード1つと分岐1つである。
    // 再開できるyieldは熱い道で戻り、LIR自身の`yielding`が`target` (オフセット0) をロードして遅い道へ進む
    let text = "\
yields fn g(x.0: tobj) -> tobj {
  return x.0
}
yields fn f(x.0: tobj) -> tobj {
  let r.1: tobj = call g(x.0)
  let y.2: int = prim yielding()
  switch y.2 { 0 -> b2, _ -> b1 }
b1:
  decref r.1
  return ()
b2:
  return r.1
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64) -> i64 tail {
        sig0 = (i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) -> i64 tail
        sig4 = (i64, i64) c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = colocated %f:g sig3
        fn4 = %eml_rt_decref_slow sig4

    block0(v0: i64, v1: i64):
        v2 = load.i64 notrap aligned v0+40
        v3 = load.i32 notrap aligned v2
        v4 = iconst.i32 0
        v5 = icmp ne v3, v4  ; v4 = 0
        v6 = get_stack_pointer.i64 
        v7 = load.i64 notrap aligned v0+48
        v8 = icmp ult v6, v7
        v9 = bor v5, v8
        brif v9, block1, block3

    block1 cold:
        v10 = func_addr.i64 fn0
        v11 = iconst.i64 0
        v12 = call fn1(v0, v10, v11)  ; v11 = 0
        v13 = iconst.i64 0
        v14 = icmp eq v12, v13  ; v13 = 0
        brif v14, block3, block2

    block2 cold:
        call fn2(v0, v1)
        v15 = iconst.i64 1
        return v15  ; v15 = 1

    block3:
        v16 = call fn3(v0, v1)
        v17 = load.i8 notrap aligned v0+24
        brif v17, block6, block7

    block6 cold:
        v18 = iconst.i64 1
        return v18  ; v18 = 1

    block7:
        v19 = load.i64 notrap aligned v0
        v20 = iconst.i64 1
        v21 = icmp ne v19, v20  ; v20 = 1
        v22 = uextend.i64 v21
        brif v22, block4, block5

    block4:
        v23 = iconst.i64 1
        v24 = band.i64 v16, v23  ; v23 = 1
        brif v24, block11, block12

    block12:
        v25 = load.i32 notrap aligned v16
        v26 = iconst.i32 -2
        v27 = iadd v25, v26  ; v26 = -2
        v28 = iconst.i32 0x7fff_fffd
        v29 = icmp ult v27, v28  ; v28 = 0x7fff_fffd
        brif v29, block8, block9

    block8:
        v30 = iconst.i32 -1
        v31 = iadd.i32 v25, v30  ; v30 = -1
        store notrap aligned v31, v16
        jump block11

    block9:
        brif.i32 v25, block10, block11

    block10 cold:
        call fn4(v0, v16)
        jump block11

    block11:
        v32 = iconst.i64 1
        return v32  ; v32 = 1

    block5:
        return v16
    }
    ");
}

#[test]
fn a_landing_point() {
    // 着地の印を持つ`apply`の後、中断の確かめの冷たいブロックは`target`をロードして印`ev.0`と比べる。着地のブロックは
    // `eml_nrt_landed`を呼び、そのダミーを結果として熱いブロックへ飛ぶ。もう一方のブロックは`eml_rt_ev_last_state`で
    // handlerの状態を取り出して手放し、フレームが持つものを手放して戻る。熱い道はほかの呼び出しと同じである
    let text = "\
effect Land { land(unit) -> tobj }
yields fn g(u.0: tobj) -> tobj {
  return u.0
}
yields fn prompt$(ev.0: ev lin, x.1: tobj) -> tobj {
  let u.2: unit = drop ev.0
  return x.1
}
fn f(w.4: evv, s.1: obj) -> obj {
  let ev.0: ev lin = evidence Land((), w.4) [&land$entry]
  let x.2: tobj = yields apply &g(()) prompt ev.0
  let r.3: tobj = call prompt$(ev.0, x.2)
  decref r.3
  return s.1
}
internal clause Land.land fn land$entry(e.0: cev, u.1: unit) -> tobj {
  decref e.0
  return ()
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64, i64) -> i64 tail {
        gv0 = symbol colocated %v:land$entry
        gv1 = symbol colocated %e:Land
        gv2 = symbol colocated %v:g
        sig0 = (i64, i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64, i64, i64, i64) -> i64 c
        sig4 = (i64, i64, i64, i64) c
        sig5 = (i64, i64, i64) -> i64 tail
        sig6 = (i64, i64, i64) -> i64 tail
        sig7 = (i64) -> i64 c
        sig8 = (i64, i64) -> i64 c
        sig9 = (i64, i64, i64) -> i64 tail
        sig10 = (i64, i64) c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = %eml_rt_ev_new sig3
        fn4 = %eml_rt_ev_set_clause sig4
        fn5 = colocated %eml$apply$1 sig6
        fn6 = %eml_nrt_landed sig7
        fn7 = %eml_rt_ev_last_state sig8
        fn8 = colocated %f:prompt$ sig9
        fn9 = %eml_rt_decref_slow sig10

    block0(v0: i64, v1: i64, v2: i64):
        v3 = load.i64 notrap aligned v0+40
        v4 = load.i32 notrap aligned v3
        v5 = iconst.i32 0
        v6 = icmp ne v4, v5  ; v5 = 0
        v7 = get_stack_pointer.i64 
        v8 = load.i64 notrap aligned v0+48
        v9 = icmp ult v7, v8
        v10 = bor v6, v9
        brif v10, block1, block3

    block1 cold:
        v11 = func_addr.i64 fn0
        v12 = iconst.i64 0
        v13 = call fn1(v0, v11, v12)  ; v12 = 0
        v14 = iconst.i64 0
        v15 = icmp eq v13, v14  ; v14 = 0
        brif v15, block3, block2

    block2 cold:
        call fn2(v0, v2)
        call fn2(v0, v1)
        v16 = iconst.i64 1
        return v16  ; v16 = 1

    block3:
        v17 = symbol_value.i64 gv0
        v18 = load.i64 notrap aligned v17
        v19 = iconst.i64 1
        v20 = symbol_value.i64 gv1
        v21 = load.i64 notrap aligned v20
        v22 = iconst.i64 1
        v23 = call fn3(v0, v21, v22, v19, v1)  ; v22 = 1, v19 = 1
        v24 = iconst.i64 0
        call fn4(v0, v23, v24, v18)  ; v24 = 0
        v25 = symbol_value.i64 gv2
        v26 = load.i64 notrap aligned v25
        v27 = iconst.i64 1
        v28 = load.i32 notrap aligned v26+4
        v29 = iconst.i32 0xffff
        v30 = band v28, v29  ; v29 = 0xffff
        v31 = iconst.i32 1
        v32 = icmp eq v30, v31  ; v31 = 1
        brif v32, block4, block5

    block4:
        v34 = iconst.i32 16
        v35 = ushr.i32 v28, v34  ; v34 = 16
        v36 = iconst.i32 255
        v37 = band v35, v36  ; v36 = 255
        v38 = iconst.i32 255
        v39 = icmp eq v37, v38  ; v38 = 255
        v40 = iconst.i64 8
        v41 = iconst.i64 16
        v42 = select v39, v41, v40  ; v41 = 16, v40 = 8
        v43 = iadd.i64 v26, v42
        v44 = load.i64 notrap aligned v43
        v45 = iconst.i64 1
        v46 = sshr v44, v45  ; v45 = 1
        v47 = call_indirect sig5, v46(v0, v26, v27)  ; v27 = 1
        jump block6(v47)

    block5:
        v48 = call fn5(v0, v26, v27)  ; v27 = 1
        jump block6(v48)

    block6(v33: i64):
        v49 = load.i8 notrap aligned v0+24
        brif v49, block7, block8(v33)

    block7 cold:
        v51 = load.i64 notrap aligned v0
        v52 = icmp eq v51, v23
        brif v52, block9, block10

    block9 cold:
        v53 = call fn6(v0)
        jump block8(v53)

    block10 cold:
        v54 = call fn7(v0, v23)
        call fn2(v0, v54)
        call fn2(v0, v23)
        call fn2(v0, v2)
        v55 = iconst.i64 1
        return v55  ; v55 = 1

    block8(v50: i64):
        v56 = call fn8(v0, v23, v50)
        v57 = load.i8 notrap aligned v0+24
        brif v57, block11, block12

    block11 cold:
        call fn2(v0, v2)
        v58 = iconst.i64 1
        return v58  ; v58 = 1

    block12:
        v59 = iconst.i64 1
        v60 = band.i64 v56, v59  ; v59 = 1
        brif v60, block16, block17

    block17:
        v61 = load.i32 notrap aligned v56
        v62 = iconst.i32 -2
        v63 = iadd v61, v62  ; v62 = -2
        v64 = iconst.i32 0x7fff_fffd
        v65 = icmp ult v63, v64  ; v64 = 0x7fff_fffd
        brif v65, block13, block14

    block13:
        v66 = iconst.i32 -1
        v67 = iadd.i32 v61, v66  ; v66 = -1
        store notrap aligned v67, v56
        jump block16

    block14:
        brif.i32 v61, block15, block16

    block15 cold:
        call fn9(v0, v56)
        jump block16

    block16:
        return v2
    }
    ");
}

/// `eml$raise`はプログラムによらない。サービスが`out`に書いた類で`Panic`か`IO`の節を決まったシグネチャで呼び、その後で
/// `abort`のバイトが立っていなければtrapする。ほかの類では何も呼ばずに戻る (docs/spec/native.mdの「externの失敗」)。
#[test]
fn the_raise_helper() {
    let text = "fn main() -> unit {\n  return ()\n}\n";
    insta::assert_snapshot!(clif(text, "eml$raise"), @"
    function %eml$raise(i64, i64, i64, i64) tail {
        ss0 = explicit_slot 32, align = 8
        sig0 = (i64, i64, i64, i64, i64) -> i64 c
        sig1 = (i64, i64, i64, i64) -> i64 tail
        sig2 = (i64, i64, i64, i64, i64) -> i64 tail
        fn0 = %eml_nrt_failed sig0

    block0(v0: i64, v1: i64, v2: i64, v3: i64):
        v4 = stack_addr.i64 ss0
        v5 = call fn0(v0, v1, v2, v3, v4)
        v6 = iconst.i64 0
        v7 = icmp eq v5, v6  ; v6 = 0
        brif v7, block3, block4

    block3:
        v8 = stack_addr.i64 ss0
        v9 = load.i64 notrap v8
        v10 = stack_addr.i64 ss0+8
        v11 = load.i64 notrap v10
        v12 = stack_addr.i64 ss0+16
        v13 = load.i64 notrap v12
        v14 = load.i32 notrap aligned v9+4
        v15 = iconst.i32 16
        v16 = ushr v14, v15  ; v15 = 16
        v17 = iconst.i32 255
        v18 = band v16, v17  ; v17 = 255
        v19 = iconst.i32 255
        v20 = icmp eq v18, v19  ; v19 = 255
        v21 = iconst.i64 8
        v22 = iconst.i64 16
        v23 = select v20, v22, v21  ; v22 = 16, v21 = 8
        v24 = iadd v9, v23
        v25 = load.i64 notrap aligned v24
        v26 = iconst.i64 1
        v27 = sshr v25, v26  ; v26 = 1
        v28 = call_indirect sig1, v27(v0, v9, v11, v13)
        v29 = load.i8 notrap aligned v0+24
        brif v29, block5, block6

    block5 cold:
        jump block1

    block6:
        jump block2

    block4:
        v30 = iconst.i64 1
        v31 = icmp.i64 eq v5, v30  ; v30 = 1
        brif v31, block7, block8

    block7:
        v32 = stack_addr.i64 ss0
        v33 = load.i64 notrap v32
        v34 = stack_addr.i64 ss0+8
        v35 = load.i64 notrap v34
        v36 = stack_addr.i64 ss0+16
        v37 = load.i64 notrap v36
        v38 = stack_addr.i64 ss0+24
        v39 = load.i64 notrap v38
        v40 = load.i32 notrap aligned v33+4
        v41 = iconst.i32 16
        v42 = ushr v40, v41  ; v41 = 16
        v43 = iconst.i32 255
        v44 = band v42, v43  ; v43 = 255
        v45 = iconst.i32 255
        v46 = icmp eq v44, v45  ; v45 = 255
        v47 = iconst.i64 8
        v48 = iconst.i64 16
        v49 = select v46, v48, v47  ; v48 = 16, v47 = 8
        v50 = iadd v33, v49
        v51 = load.i64 notrap aligned v50
        v52 = iconst.i64 1
        v53 = sshr v51, v52  ; v52 = 1
        v54 = call_indirect sig2, v53(v0, v33, v35, v37, v39)
        v55 = load.i8 notrap aligned v0+24
        brif v55, block9, block10

    block9 cold:
        jump block1

    block10:
        jump block2

    block8:
        jump block1

    block2 cold:
        trap user1

    block1:
        return
    }
    ");
}

/// 欄0を残し欄1を手放す`release`。`f`はtokenを作らない形で、`g`はtokenを作る形である。
const RELEASES: &str = "\
layout P { P(obj, obj, int) }
fn f(x.0: obj) -> obj {
  unpack x.0 P #0(a.1: obj, b.2: obj, n.3: int)
  release x.0 P #0(a.1, _, _)
  return a.1
}
fn g(x.0: obj) -> obj {
  unpack x.0 P #0(a.1: obj, b.2: obj, n.3: int)
  let r.4: token = release x.0 P #0(a.1, _, _)
  drop_token r.4
  return a.1
}
";

#[test]
fn a_release() {
    // 生成したコードは一意な道だけを持つ。数が1なら、残さない欄1をインラインの`decref`で手放し、箱を空けるか箱の語を
    // tokenにする。ほかの数では、冷たいブロックで`eml_rt_release(ctx, x, k:10, 2)`を1回呼び、tokenは空である
    // (docs/spec/native.md の「データ」)
    insta::assert_snapshot!(clif(RELEASES, "f"), @"
    function %f:f(i64, i64) -> i64 tail {
        gv0 = symbol colocated %k:10
        sig0 = (i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64) c
        sig5 = (i64, i64, i64, i64) c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = %eml_rt_decref_slow sig3
        fn4 = %eml_rt_free_block sig4
        fn5 = %eml_rt_release sig5

    block0(v0: i64, v1: i64):
        v2 = load.i64 notrap aligned v0+40
        v3 = load.i32 notrap aligned v2
        v4 = iconst.i32 0
        v5 = icmp ne v3, v4  ; v4 = 0
        v6 = get_stack_pointer.i64 
        v7 = load.i64 notrap aligned v0+48
        v8 = icmp ult v6, v7
        v9 = bor v5, v8
        brif v9, block1, block3

    block1 cold:
        v10 = func_addr.i64 fn0
        v11 = iconst.i64 0
        v12 = call fn1(v0, v10, v11)  ; v11 = 0
        v13 = iconst.i64 0
        v14 = icmp eq v12, v13  ; v13 = 0
        brif v14, block3, block2

    block2 cold:
        call fn2(v0, v1)
        v15 = iconst.i64 1
        return v15  ; v15 = 1

    block3:
        v16 = load.i64 notrap aligned v1+8
        v17 = load.i64 notrap aligned v1+16
        v18 = load.i64 notrap aligned v1+24
        v19 = load.i32 notrap aligned v1
        v20 = iconst.i32 1
        v21 = icmp eq v19, v20  ; v20 = 1
        brif v21, block5, block6

    block5:
        v22 = load.i64 notrap aligned v1+16
        v23 = load.i32 notrap aligned v22
        v24 = iconst.i32 -2
        v25 = iadd v23, v24  ; v24 = -2
        v26 = iconst.i32 0x7fff_fffd
        v27 = icmp ult v25, v26  ; v26 = 0x7fff_fffd
        brif v27, block7, block8

    block7:
        v28 = iconst.i32 -1
        v29 = iadd.i32 v23, v28  ; v28 = -1
        store notrap aligned v29, v22
        jump block10

    block8:
        brif.i32 v23, block9, block10

    block9 cold:
        call fn3(v0, v22)
        jump block10

    block10:
        call fn4(v0, v1)
        jump block4

    block6 cold:
        v30 = symbol_value.i64 gv0
        v31 = iconst.i64 2
        call fn5(v0, v1, v30, v31)  ; v31 = 2
        jump block4

    block4:
        return v16
    }
    ");
    insta::assert_snapshot!(clif(RELEASES, "g"), @"
    function %f:g(i64, i64) -> i64 tail {
        gv0 = symbol colocated %k:10
        sig0 = (i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64, i64, i64) c
        sig5 = (i64, i64) c
        fn0 = colocated %f:g sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = %eml_rt_decref_slow sig3
        fn4 = %eml_rt_release sig4
        fn5 = %eml_rt_drop_token sig5

    block0(v0: i64, v1: i64):
        v2 = load.i64 notrap aligned v0+40
        v3 = load.i32 notrap aligned v2
        v4 = iconst.i32 0
        v5 = icmp ne v3, v4  ; v4 = 0
        v6 = get_stack_pointer.i64 
        v7 = load.i64 notrap aligned v0+48
        v8 = icmp ult v6, v7
        v9 = bor v5, v8
        brif v9, block1, block3

    block1 cold:
        v10 = func_addr.i64 fn0
        v11 = iconst.i64 0
        v12 = call fn1(v0, v10, v11)  ; v11 = 0
        v13 = iconst.i64 0
        v14 = icmp eq v12, v13  ; v13 = 0
        brif v14, block3, block2

    block2 cold:
        call fn2(v0, v1)
        v15 = iconst.i64 1
        return v15  ; v15 = 1

    block3:
        v16 = load.i64 notrap aligned v1+8
        v17 = load.i64 notrap aligned v1+16
        v18 = load.i64 notrap aligned v1+24
        v20 = load.i32 notrap aligned v1
        v21 = iconst.i32 1
        v22 = icmp eq v20, v21  ; v21 = 1
        brif v22, block5, block6

    block5:
        v23 = load.i64 notrap aligned v1+16
        v24 = load.i32 notrap aligned v23
        v25 = iconst.i32 -2
        v26 = iadd v24, v25  ; v25 = -2
        v27 = iconst.i32 0x7fff_fffd
        v28 = icmp ult v26, v27  ; v27 = 0x7fff_fffd
        brif v28, block7, block8

    block7:
        v29 = iconst.i32 -1
        v30 = iadd.i32 v24, v29  ; v29 = -1
        store notrap aligned v30, v23
        jump block10

    block8:
        brif.i32 v24, block9, block10

    block9 cold:
        call fn3(v0, v23)
        jump block10

    block10:
        jump block4(v1)

    block6 cold:
        v31 = symbol_value.i64 gv0
        v32 = iconst.i64 2
        call fn4(v0, v1, v31, v32)  ; v32 = 2
        v33 = iconst.i64 0
        jump block4(v33)  ; v33 = 0

    block4(v19: i64):
        call fn5(v0, v19)
        return v16
    }
    ");
}

#[test]
fn keep_bytes_are_read_only_and_not_loaded() {
    // `keep`のバイトは中身で名前を決めた読み取り専用の物で、ローダーは書かないので`Image.data`に行を持たない。物は0を
    // 足して8バイトの倍数かつ8バイト以上にする。生の欄だけの構築子のtokenを作る`release`は何も残さず、走査する語も
    // ないので、`k:`の8バイトの0を渡す。`f`と`g`は同じ並びなので、物`k:10`を1つ共有する (docs/spec/native.md の
    // 「値と定数」)
    let fields = ["obj"; 9].join(", ");
    let bound: Vec<String> = (0..9).map(|i| format!("a{i}.{}: obj", i + 1)).collect();
    let text = format!(
        "layout N {{ N({fields}) }}
layout S {{ S(int, int) }}
{RELEASES}fn h(x.0: obj) -> obj {{
  unpack x.0 N #0({bound})
  release x.0 N #0(a0.1, _, _, _, _, _, _, _, a8.9)
  decref a8.9
  return a0.1
}}
fn s(x.0: obj) -> int {{
  unpack x.0 S #0(n.1: int, m.2: int)
  let r.3: token = release x.0 S #0(_, _)
  drop_token r.3
  let c.4: obj = const \"c\"
  decref c.4
  return n.1
}}
",
        bound = bound.join(", ")
    );
    let mut m = machine();
    m.load(&program(&text), &Linked::default()).unwrap();
    let symbols = m.data_symbols();
    let mut keeps: Vec<(String, *const u8, Vec<u8>)> = symbols
        .iter()
        .filter(|(name, _, _)| name.starts_with("k:"))
        .map(|&(ref name, at, writable)| {
            assert!(!writable, "{name}");
            let size = if name.len() > 2 + 8 { 16 } else { 8 };
            // SAFETY: マシンが生きている間生きている、`size`バイトの物である
            let bytes = unsafe { std::slice::from_raw_parts(at, size) }.to_vec();
            (name.clone(), at, bytes)
        })
        .collect();
    keeps.sort_by(|a, b| a.0.cmp(&b.0));
    let mut nine = vec![0; 16];
    (nine[0], nine[8]) = (1, 1);
    assert_eq!(
        keeps
            .iter()
            .map(|(name, _, bytes)| (name.as_str(), bytes.clone()))
            .collect::<Vec<_>>(),
        [
            ("k:", vec![0; 8]),
            ("k:10", vec![1, 0, 0, 0, 0, 0, 0, 0]),
            ("k:100000001", nine)
        ]
    );
    let image = m.image().unwrap();
    // SAFETY: マシンが生きている間、`Image`の表は生きていて変わらない
    let data = unsafe { image.data.as_slice() };
    assert_eq!(data.len(), symbols.len() - keeps.len());
    for row in data {
        let slot = row.slot.cast_const().cast::<u8>();
        assert!(keeps.iter().all(|&(_, at, _)| slot != at));
    }
    m.close();
}

#[test]
fn a_con() {
    // `con`は`eml_rt_alloc_data`を呼び、各欄を決まったずれにインラインでストアする。生の`float`の欄はそのビット列で
    // ストアする。検査モードのコードは、各ストアの前に同じ語のストアの監査を置く
    let text = "\
layout P { P(obj, int, float) }
fn f(s.0: obj, n.1: int, x.2: float) -> obj {
  let p.3: obj = con P #0(s.0, n.1, x.2)
  return p.3
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64, i64, f64) -> i64 tail {
        sig0 = (i64, i64, i64, f64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64, i64, i64, i64) -> i64 c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = %eml_rt_alloc_data sig3

    block0(v0: i64, v1: i64, v2: i64, v3: f64):
        v4 = load.i64 notrap aligned v0+40
        v5 = load.i32 notrap aligned v4
        v6 = iconst.i32 0
        v7 = icmp ne v5, v6  ; v6 = 0
        v8 = get_stack_pointer.i64 
        v9 = load.i64 notrap aligned v0+48
        v10 = icmp ult v8, v9
        v11 = bor v7, v10
        brif v11, block1, block3

    block1 cold:
        v12 = func_addr.i64 fn0
        v13 = iconst.i64 0
        v14 = call fn1(v0, v12, v13)  ; v13 = 0
        v15 = iconst.i64 0
        v16 = icmp eq v14, v15  ; v15 = 0
        brif v16, block3, block2

    block2 cold:
        call fn2(v0, v1)
        v17 = iconst.i64 1
        return v17  ; v17 = 1

    block3:
        v18 = iconst.i64 0
        v19 = iconst.i64 1
        v20 = iconst.i64 2
        v21 = iconst.i64 0
        v22 = call fn3(v0, v18, v19, v20, v21)  ; v18 = 0, v19 = 1, v20 = 2, v21 = 0
        store.i64 notrap aligned v1, v22+8
        store.i64 notrap aligned v2, v22+16
        store.f64 notrap aligned v3, v22+24
        return v22
    }
    ");
    insta::assert_snapshot!(twin(text, "f"), @"
    function %f:f(i64, i64, i64, f64) -> i64 tail {
        sig0 = (i64, i64, i64, f64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64, i64, i64, i64) -> i64 c
        sig5 = (i64, i64, i64, i64) c
        sig6 = (i64, i64, i64, i64) c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_audit_count sig2
        fn3 = %eml_rt_decref sig3
        fn4 = %eml_rt_alloc_data sig4
        fn5 = %eml_rt_audit_store sig5
        fn6 = %eml_rt_audit_store_raw sig6

    block0(v0: i64, v1: i64, v2: i64, v3: f64):
        v4 = load.i64 notrap aligned v0+40
        v5 = load.i32 notrap aligned v4
        v6 = iconst.i32 0
        v7 = icmp ne v5, v6  ; v6 = 0
        v8 = get_stack_pointer.i64 
        v9 = load.i64 notrap aligned v0+48
        v10 = icmp ult v8, v9
        v11 = bor v7, v10
        brif v11, block1, block3

    block1 cold:
        v12 = func_addr.i64 fn0
        v13 = iconst.i64 0
        v14 = call fn1(v0, v12, v13)  ; v13 = 0
        v15 = iconst.i64 0
        v16 = icmp eq v14, v15  ; v15 = 0
        brif v16, block3, block2

    block2 cold:
        v17 = iconst.i64 2
        call fn2(v0, v17)  ; v17 = 2
        call fn3(v0, v1)
        v18 = iconst.i64 1
        return v18  ; v18 = 1

    block3:
        v19 = iconst.i64 0
        v20 = iconst.i64 1
        v21 = iconst.i64 2
        v22 = iconst.i64 0
        v23 = call fn4(v0, v19, v20, v21, v22)  ; v19 = 0, v20 = 1, v21 = 2, v22 = 0
        v24 = iconst.i64 0
        call fn5(v0, v23, v24, v1)  ; v24 = 0
        store.i64 notrap aligned v1, v23+8
        v25 = iconst.i64 1
        call fn6(v0, v23, v25, v2)  ; v25 = 1
        store.i64 notrap aligned v2, v23+16
        v26 = iconst.i64 2
        v27 = bitcast.i64 v3
        call fn6(v0, v23, v26, v27)  ; v26 = 2
        store.f64 notrap aligned v3, v23+24
        return v23
    }
    ");
}

#[test]
fn a_reused_con() {
    // `f`はtokenをその場でreuseする。速いコードは箱の語をtokenに、tokenを箱にし、検査モードのコードは
    // `eml_rt_into_token`と`eml_rt_reuse_in_place`を呼ぶ。`g`は`eml_rt_con_at`でヘッダを書き直す。どちらも書かない印の
    // ない欄だけを書き、検査モードのコードはreuseした腕で`reuses`を数える
    let text = "\
layout P { P(obj, obj, int) }
layout T { A(obj, int), B(int, obj) }
fn f(x.0: obj, c.1: obj) -> obj {
  unpack x.0 P #0(a.2: obj, b.3: obj, n.4: int)
  let r.5: token = release x.0 P #0(a.2, _, _)
  let d.6: obj = con P #0(=a.2, c.1, =n.4) reuse r.5 in place
  return d.6
}
fn g(x.0: obj) -> obj {
  switch x.0 T { #0(a.1: obj, n.2: int) -> b1, #1(m.3: int, c.4: obj) -> b2 }
b1:
  let r.5: token = release x.0 T #0(a.1, _)
  let y.6: obj = con T #1(n.2, a.1) reuse r.5
  return y.6
b2:
  let r.7: token = release x.0 T #1(_, c.4)
  let y.8: obj = con T #0(c.4, m.3) reuse r.7
  return y.8
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64, i64) -> i64 tail {
        gv0 = symbol colocated %k:10
        sig0 = (i64, i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64, i64, i64) c
        sig5 = (i64, i64, i64, i64, i64) -> i64 c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = %eml_rt_decref_slow sig3
        fn4 = %eml_rt_release sig4
        fn5 = %eml_rt_alloc_data sig5

    block0(v0: i64, v1: i64, v2: i64):
        v3 = load.i64 notrap aligned v0+40
        v4 = load.i32 notrap aligned v3
        v5 = iconst.i32 0
        v6 = icmp ne v4, v5  ; v5 = 0
        v7 = get_stack_pointer.i64 
        v8 = load.i64 notrap aligned v0+48
        v9 = icmp ult v7, v8
        v10 = bor v6, v9
        brif v10, block1, block3

    block1 cold:
        v11 = func_addr.i64 fn0
        v12 = iconst.i64 0
        v13 = call fn1(v0, v11, v12)  ; v12 = 0
        v14 = iconst.i64 0
        v15 = icmp eq v13, v14  ; v14 = 0
        brif v15, block3, block2

    block2 cold:
        call fn2(v0, v1)
        call fn2(v0, v2)
        v16 = iconst.i64 1
        return v16  ; v16 = 1

    block3:
        v17 = load.i64 notrap aligned v1+8
        v18 = load.i64 notrap aligned v1+16
        v19 = load.i64 notrap aligned v1+24
        v21 = load.i32 notrap aligned v1
        v22 = iconst.i32 1
        v23 = icmp eq v21, v22  ; v22 = 1
        brif v23, block5, block6

    block5:
        v24 = load.i64 notrap aligned v1+16
        v25 = load.i32 notrap aligned v24
        v26 = iconst.i32 -2
        v27 = iadd v25, v26  ; v26 = -2
        v28 = iconst.i32 0x7fff_fffd
        v29 = icmp ult v27, v28  ; v28 = 0x7fff_fffd
        brif v29, block7, block8

    block7:
        v30 = iconst.i32 -1
        v31 = iadd.i32 v25, v30  ; v30 = -1
        store notrap aligned v31, v24
        jump block10

    block8:
        brif.i32 v25, block9, block10

    block9 cold:
        call fn3(v0, v24)
        jump block10

    block10:
        jump block4(v1)

    block6 cold:
        v32 = symbol_value.i64 gv0
        v33 = iconst.i64 2
        call fn4(v0, v1, v32, v33)  ; v33 = 2
        v34 = iconst.i64 0
        jump block4(v34)  ; v34 = 0

    block4(v20: i64):
        v35 = iconst.i64 0
        v36 = iconst.i64 2
        v37 = iconst.i64 1
        v38 = iconst.i64 0
        brif v20, block12, block11

    block11:
        v40 = call fn5(v0, v35, v36, v37, v38)  ; v35 = 0, v36 = 2, v37 = 1, v38 = 0
        store.i64 notrap aligned v17, v40+8
        store.i64 notrap aligned v2, v40+16
        store.i64 notrap aligned v19, v40+24
        jump block13(v40)

    block12:
        store.i64 notrap aligned v2, v20+16
        jump block13(v20)

    block13(v39: i64):
        return v39
    }
    ");
    insta::assert_snapshot!(twin(text, "f"), @"
    function %f:f(i64, i64, i64) -> i64 tail {
        gv0 = symbol colocated %k:10
        sig0 = (i64, i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64, i64, i64, i64) c
        sig5 = (i64, i64, i64) c
        sig6 = (i64, i64, i64) c
        sig7 = (i64, i64) c
        sig8 = (i64, i64) c
        sig9 = (i64, i64) c
        sig10 = (i64, i64) -> i64 c
        sig11 = (i64, i64, i64, i64) c
        sig12 = (i64, i64, i64, i64, i64) -> i64 c
        sig13 = (i64, i64, i64, i64) c
        sig14 = (i64, i64, i64, i64) c
        sig15 = (i64, i64, i64, i64, i64) -> i64 c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_audit_count sig2
        fn3 = %eml_rt_decref sig3
        fn4 = %eml_rt_audit_data sig4
        fn5 = %eml_rt_audit_field sig5
        fn6 = %eml_rt_audit_raw sig6
        fn7 = %eml_rt_audit_unique sig7
        fn8 = %eml_rt_decref_slow sig8
        fn9 = %eml_rt_audit_decref sig9
        fn10 = %eml_rt_into_token sig10
        fn11 = %eml_rt_release sig11
        fn12 = %eml_rt_alloc_data sig12
        fn13 = %eml_rt_audit_store sig13
        fn14 = %eml_rt_audit_store_raw sig14
        fn15 = %eml_rt_reuse_in_place sig15

    block0(v0: i64, v1: i64, v2: i64):
        v3 = load.i64 notrap aligned v0+40
        v4 = load.i32 notrap aligned v3
        v5 = iconst.i32 0
        v6 = icmp ne v4, v5  ; v5 = 0
        v7 = get_stack_pointer.i64 
        v8 = load.i64 notrap aligned v0+48
        v9 = icmp ult v7, v8
        v10 = bor v6, v9
        brif v10, block1, block3

    block1 cold:
        v11 = func_addr.i64 fn0
        v12 = iconst.i64 0
        v13 = call fn1(v0, v11, v12)  ; v12 = 0
        v14 = iconst.i64 0
        v15 = icmp eq v13, v14  ; v14 = 0
        brif v15, block3, block2

    block2 cold:
        v16 = iconst.i64 2
        call fn2(v0, v16)  ; v16 = 2
        call fn3(v0, v1)
        call fn3(v0, v2)
        v17 = iconst.i64 1
        return v17  ; v17 = 1

    block3:
        v18 = iconst.i64 0
        v19 = iconst.i64 2
        v20 = iconst.i64 0
        call fn4(v0, v1, v18, v19, v20)  ; v18 = 0, v19 = 2, v20 = 0
        v21 = iconst.i64 0
        call fn5(v0, v1, v21)  ; v21 = 0
        v22 = load.i64 notrap aligned v1+8
        v23 = iconst.i64 1
        call fn5(v0, v1, v23)  ; v23 = 1
        v24 = load.i64 notrap aligned v1+16
        v25 = iconst.i64 2
        call fn6(v0, v1, v25)  ; v25 = 2
        v26 = load.i64 notrap aligned v1+24
        v28 = iconst.i64 0
        v29 = iconst.i64 2
        v30 = iconst.i64 2
        call fn4(v0, v1, v28, v29, v30)  ; v28 = 0, v29 = 2, v30 = 2
        call fn7(v0, v1)
        v31 = load.i32 notrap aligned v1
        v32 = iconst.i32 1
        v33 = icmp eq v31, v32  ; v32 = 1
        brif v33, block5, block6

    block5:
        v34 = iconst.i64 1
        call fn5(v0, v1, v34)  ; v34 = 1
        v35 = load.i64 notrap aligned v1+16
        call fn9(v0, v35)
        v36 = load.i32 notrap aligned v35
        v37 = iconst.i32 -2
        v38 = iadd v36, v37  ; v37 = -2
        v39 = iconst.i32 0x7fff_fffd
        v40 = icmp ult v38, v39  ; v39 = 0x7fff_fffd
        brif v40, block7, block8

    block7:
        v41 = iconst.i32 -1
        v42 = iadd.i32 v36, v41  ; v41 = -1
        store notrap aligned v42, v35
        jump block10

    block8:
        brif.i32 v36, block9, block10

    block9 cold:
        call fn8(v0, v35)
        jump block10

    block10:
        v43 = call fn10(v0, v1)
        jump block4(v43)

    block6 cold:
        v44 = symbol_value.i64 gv0
        v45 = iconst.i64 2
        call fn11(v0, v1, v44, v45)  ; v45 = 2
        v46 = iconst.i64 0
        jump block4(v46)  ; v46 = 0

    block4(v27: i64):
        v47 = iconst.i64 0
        v48 = iconst.i64 2
        v49 = iconst.i64 1
        v50 = iconst.i64 0
        brif v27, block12, block11

    block11:
        v52 = call fn12(v0, v47, v48, v49, v50)  ; v47 = 0, v48 = 2, v49 = 1, v50 = 0
        v53 = iconst.i64 0
        call fn13(v0, v52, v53, v22)  ; v53 = 0
        store.i64 notrap aligned v22, v52+8
        v54 = iconst.i64 1
        call fn13(v0, v52, v54, v2)  ; v54 = 1
        store.i64 notrap aligned v2, v52+16
        v55 = iconst.i64 2
        call fn14(v0, v52, v55, v26)  ; v55 = 2
        store.i64 notrap aligned v26, v52+24
        jump block13(v52)

    block12:
        v56 = iconst.i64 3
        call fn2(v0, v56)  ; v56 = 3
        v57 = call fn15(v0, v27, v47, v48, v50)  ; v47 = 0, v48 = 2, v50 = 0
        v58 = iconst.i64 1
        call fn13(v0, v57, v58, v2)  ; v58 = 1
        store.i64 notrap aligned v2, v57+16
        jump block13(v57)

    block13(v51: i64):
        return v51
    }
    ");
    insta::assert_snapshot!(clif(text, "g"), @"
    function %f:g(i64, i64) -> i64 tail {
        gv0 = symbol colocated %k:1
        sig0 = (i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64, i64, i64) c
        sig4 = (i64, i64, i64, i64, i64) -> i64 c
        sig5 = (i64, i64, i64, i64, i64, i64) -> i64 c
        fn0 = colocated %f:g sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = %eml_rt_release sig3
        fn4 = %eml_rt_alloc_data sig4
        fn5 = %eml_rt_con_at sig5

    block0(v0: i64, v1: i64):
        v2 = load.i64 notrap aligned v0+40
        v3 = load.i32 notrap aligned v2
        v4 = iconst.i32 0
        v5 = icmp ne v3, v4  ; v4 = 0
        v6 = get_stack_pointer.i64 
        v7 = load.i64 notrap aligned v0+48
        v8 = icmp ult v6, v7
        v9 = bor v5, v8
        brif v9, block1, block3

    block1 cold:
        v10 = func_addr.i64 fn0
        v11 = iconst.i64 0
        v12 = call fn1(v0, v10, v11)  ; v11 = 0
        v13 = iconst.i64 0
        v14 = icmp eq v12, v13  ; v13 = 0
        brif v14, block3, block2

    block2 cold:
        call fn2(v0, v1)
        v15 = iconst.i64 1
        return v15  ; v15 = 1

    block3:
        v16 = load.i32 notrap aligned v1+4
        v17 = iconst.i32 0xffff
        v18 = band v16, v17  ; v17 = 0xffff
        br_table v18, block6, [block4, block5]

    block6 cold:
        trap user1

    block4:
        v19 = load.i64 notrap aligned v1+8
        v20 = load.i64 notrap aligned v1+16
        v22 = load.i32 notrap aligned v1
        v23 = iconst.i32 1
        v24 = icmp eq v22, v23  ; v23 = 1
        brif v24, block8, block9

    block8:
        jump block7(v1)

    block9 cold:
        v25 = symbol_value.i64 gv0
        v26 = iconst.i64 1
        call fn3(v0, v1, v25, v26)  ; v26 = 1
        v27 = iconst.i64 0
        jump block7(v27)  ; v27 = 0

    block7(v21: i64):
        v28 = iconst.i64 1
        v29 = iconst.i64 1
        v30 = iconst.i64 1
        v31 = iconst.i64 0
        brif v21, block11, block10

    block10:
        v33 = call fn4(v0, v28, v29, v30, v31)  ; v28 = 1, v29 = 1, v30 = 1, v31 = 0
        store.i64 notrap aligned v20, v33+16
        store.i64 notrap aligned v19, v33+8
        jump block12(v33)

    block11:
        v34 = call fn5(v0, v21, v28, v29, v30, v31)  ; v28 = 1, v29 = 1, v30 = 1, v31 = 0
        store.i64 notrap aligned v20, v34+16
        store.i64 notrap aligned v19, v34+8
        jump block12(v34)

    block12(v32: i64):
        return v32

    block5:
        v35 = load.i64 notrap aligned v1+16
        v36 = load.i64 notrap aligned v1+8
        v38 = load.i32 notrap aligned v1
        v39 = iconst.i32 1
        v40 = icmp eq v38, v39  ; v39 = 1
        brif v40, block14, block15

    block14:
        jump block13(v1)

    block15 cold:
        v41 = symbol_value.i64 gv0
        v42 = iconst.i64 1
        call fn3(v0, v1, v41, v42)  ; v42 = 1
        v43 = iconst.i64 0
        jump block13(v43)  ; v43 = 0

    block13(v37: i64):
        v44 = iconst.i64 0
        v45 = iconst.i64 1
        v46 = iconst.i64 1
        v47 = iconst.i64 0
        brif v37, block17, block16

    block16:
        v49 = call fn4(v0, v44, v45, v46, v47)  ; v44 = 0, v45 = 1, v46 = 1, v47 = 0
        store.i64 notrap aligned v36, v49+8
        store.i64 notrap aligned v35, v49+16
        jump block18(v49)

    block17:
        v50 = call fn5(v0, v37, v44, v45, v46, v47)  ; v44 = 0, v45 = 1, v46 = 1, v47 = 0
        store.i64 notrap aligned v36, v50+8
        store.i64 notrap aligned v35, v50+16
        jump block18(v50)

    block18(v48: i64):
        return v48
    }
    ");
    insta::assert_snapshot!(twin(text, "g"), @"
    function %f:g(i64, i64) -> i64 tail {
        gv0 = symbol colocated %k:1
        sig0 = (i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64) c
        sig5 = (i64, i64, i64, i64, i64) c
        sig6 = (i64, i64, i64) c
        sig7 = (i64, i64, i64) c
        sig8 = (i64, i64) c
        sig9 = (i64, i64) -> i64 c
        sig10 = (i64, i64, i64, i64) c
        sig11 = (i64, i64, i64, i64, i64) -> i64 c
        sig12 = (i64, i64, i64, i64) c
        sig13 = (i64, i64, i64, i64) c
        sig14 = (i64, i64, i64, i64, i64, i64) -> i64 c
        fn0 = colocated %f:g sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_audit_count sig2
        fn3 = %eml_rt_decref sig3
        fn4 = %eml_rt_audit_header sig4
        fn5 = %eml_rt_audit_data sig5
        fn6 = %eml_rt_audit_field sig6
        fn7 = %eml_rt_audit_raw sig7
        fn8 = %eml_rt_audit_unique sig8
        fn9 = %eml_rt_into_token sig9
        fn10 = %eml_rt_release sig10
        fn11 = %eml_rt_alloc_data sig11
        fn12 = %eml_rt_audit_store_raw sig12
        fn13 = %eml_rt_audit_store sig13
        fn14 = %eml_rt_con_at sig14

    block0(v0: i64, v1: i64):
        v2 = load.i64 notrap aligned v0+40
        v3 = load.i32 notrap aligned v2
        v4 = iconst.i32 0
        v5 = icmp ne v3, v4  ; v4 = 0
        v6 = get_stack_pointer.i64 
        v7 = load.i64 notrap aligned v0+48
        v8 = icmp ult v6, v7
        v9 = bor v5, v8
        brif v9, block1, block3

    block1 cold:
        v10 = func_addr.i64 fn0
        v11 = iconst.i64 0
        v12 = call fn1(v0, v10, v11)  ; v11 = 0
        v13 = iconst.i64 0
        v14 = icmp eq v12, v13  ; v13 = 0
        brif v14, block3, block2

    block2 cold:
        v15 = iconst.i64 2
        call fn2(v0, v15)  ; v15 = 2
        call fn3(v0, v1)
        v16 = iconst.i64 1
        return v16  ; v16 = 1

    block3:
        call fn4(v0, v1)
        v17 = load.i32 notrap aligned v1+4
        v18 = iconst.i32 0xffff
        v19 = band v17, v18  ; v18 = 0xffff
        br_table v19, block6, [block4, block5]

    block6 cold:
        trap user1

    block4:
        v20 = iconst.i64 0
        v21 = iconst.i64 1
        v22 = iconst.i64 1
        call fn5(v0, v1, v20, v21, v22)  ; v20 = 0, v21 = 1, v22 = 1
        v23 = iconst.i64 0
        call fn6(v0, v1, v23)  ; v23 = 0
        v24 = load.i64 notrap aligned v1+8
        v25 = iconst.i64 1
        call fn7(v0, v1, v25)  ; v25 = 1
        v26 = load.i64 notrap aligned v1+16
        v28 = iconst.i64 0
        v29 = iconst.i64 1
        v30 = iconst.i64 2
        call fn5(v0, v1, v28, v29, v30)  ; v28 = 0, v29 = 1, v30 = 2
        call fn8(v0, v1)
        v31 = load.i32 notrap aligned v1
        v32 = iconst.i32 1
        v33 = icmp eq v31, v32  ; v32 = 1
        brif v33, block8, block9

    block8:
        v34 = call fn9(v0, v1)
        jump block7(v34)

    block9 cold:
        v35 = symbol_value.i64 gv0
        v36 = iconst.i64 1
        call fn10(v0, v1, v35, v36)  ; v36 = 1
        v37 = iconst.i64 0
        jump block7(v37)  ; v37 = 0

    block7(v27: i64):
        v38 = iconst.i64 1
        v39 = iconst.i64 1
        v40 = iconst.i64 1
        v41 = iconst.i64 0
        brif v27, block11, block10

    block10:
        v43 = call fn11(v0, v38, v39, v40, v41)  ; v38 = 1, v39 = 1, v40 = 1, v41 = 0
        v44 = iconst.i64 1
        call fn12(v0, v43, v44, v26)  ; v44 = 1
        store.i64 notrap aligned v26, v43+16
        v45 = iconst.i64 0
        call fn13(v0, v43, v45, v24)  ; v45 = 0
        store.i64 notrap aligned v24, v43+8
        jump block12(v43)

    block11:
        v46 = iconst.i64 3
        call fn2(v0, v46)  ; v46 = 3
        v47 = call fn14(v0, v27, v38, v39, v40, v41)  ; v38 = 1, v39 = 1, v40 = 1, v41 = 0
        v48 = iconst.i64 1
        call fn12(v0, v47, v48, v26)  ; v48 = 1
        store.i64 notrap aligned v26, v47+16
        v49 = iconst.i64 0
        call fn13(v0, v47, v49, v24)  ; v49 = 0
        store.i64 notrap aligned v24, v47+8
        jump block12(v47)

    block12(v42: i64):
        return v42

    block5:
        v50 = iconst.i64 1
        v51 = iconst.i64 1
        v52 = iconst.i64 1
        call fn5(v0, v1, v50, v51, v52)  ; v50 = 1, v51 = 1, v52 = 1
        v53 = iconst.i64 1
        call fn7(v0, v1, v53)  ; v53 = 1
        v54 = load.i64 notrap aligned v1+16
        v55 = iconst.i64 0
        call fn6(v0, v1, v55)  ; v55 = 0
        v56 = load.i64 notrap aligned v1+8
        v58 = iconst.i64 1
        v59 = iconst.i64 1
        v60 = iconst.i64 2
        call fn5(v0, v1, v58, v59, v60)  ; v58 = 1, v59 = 1, v60 = 2
        call fn8(v0, v1)
        v61 = load.i32 notrap aligned v1
        v62 = iconst.i32 1
        v63 = icmp eq v61, v62  ; v62 = 1
        brif v63, block14, block15

    block14:
        v64 = call fn9(v0, v1)
        jump block13(v64)

    block15 cold:
        v65 = symbol_value.i64 gv0
        v66 = iconst.i64 1
        call fn10(v0, v1, v65, v66)  ; v66 = 1
        v67 = iconst.i64 0
        jump block13(v67)  ; v67 = 0

    block13(v57: i64):
        v68 = iconst.i64 0
        v69 = iconst.i64 1
        v70 = iconst.i64 1
        v71 = iconst.i64 0
        brif v57, block17, block16

    block16:
        v73 = call fn11(v0, v68, v69, v70, v71)  ; v68 = 0, v69 = 1, v70 = 1, v71 = 0
        v74 = iconst.i64 0
        call fn13(v0, v73, v74, v56)  ; v74 = 0
        store.i64 notrap aligned v56, v73+8
        v75 = iconst.i64 1
        call fn12(v0, v73, v75, v54)  ; v75 = 1
        store.i64 notrap aligned v54, v73+16
        jump block18(v73)

    block17:
        v76 = iconst.i64 3
        call fn2(v0, v76)  ; v76 = 3
        v77 = call fn14(v0, v57, v68, v69, v70, v71)  ; v68 = 0, v69 = 1, v70 = 1, v71 = 0
        v78 = iconst.i64 0
        call fn13(v0, v77, v78, v56)  ; v78 = 0
        store.i64 notrap aligned v56, v77+8
        v79 = iconst.i64 1
        call fn12(v0, v77, v79, v54)  ; v79 = 1
        store.i64 notrap aligned v54, v77+16
        jump block18(v77)

    block18(v72: i64):
        return v72
    }
    ");
}

#[test]
fn an_unpack() {
    // `unpack`は、各欄を決まったずれのロード1つで読む。検査モードのコードは、先にタグと走査する語の数を監査し、次に各欄を
    // 監査する
    let text = "\
layout P { P(obj, int) }
fn f(borrowed p.0: obj) -> int {
  unpack p.0 P #0(s.1: obj, n.2: int)
  return n.2
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64) -> i64 tail {
        sig0 = (i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1

    block0(v0: i64, v1: i64):
        v2 = load.i64 notrap aligned v0+40
        v3 = load.i32 notrap aligned v2
        v4 = iconst.i32 0
        v5 = icmp ne v3, v4  ; v4 = 0
        v6 = get_stack_pointer.i64 
        v7 = load.i64 notrap aligned v0+48
        v8 = icmp ult v6, v7
        v9 = bor v5, v8
        brif v9, block1, block3

    block1 cold:
        v10 = func_addr.i64 fn0
        v11 = iconst.i64 0
        v12 = call fn1(v0, v10, v11)  ; v11 = 0
        v13 = iconst.i64 0
        v14 = icmp eq v12, v13  ; v13 = 0
        brif v14, block3, block2

    block2 cold:
        v15 = iconst.i64 0
        return v15  ; v15 = 0

    block3:
        v16 = load.i64 notrap aligned v1+8
        v17 = load.i64 notrap aligned v1+16
        return v17
    }
    ");
    insta::assert_snapshot!(twin(text, "f"), @"
    function %f:f(i64, i64) -> i64 tail {
        sig0 = (i64, i64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64, i64, i64, i64) c
        sig4 = (i64, i64, i64) c
        sig5 = (i64, i64, i64) c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_audit_count sig2
        fn3 = %eml_rt_audit_data sig3
        fn4 = %eml_rt_audit_field sig4
        fn5 = %eml_rt_audit_raw sig5

    block0(v0: i64, v1: i64):
        v2 = load.i64 notrap aligned v0+40
        v3 = load.i32 notrap aligned v2
        v4 = iconst.i32 0
        v5 = icmp ne v3, v4  ; v4 = 0
        v6 = get_stack_pointer.i64 
        v7 = load.i64 notrap aligned v0+48
        v8 = icmp ult v6, v7
        v9 = bor v5, v8
        brif v9, block1, block3

    block1 cold:
        v10 = func_addr.i64 fn0
        v11 = iconst.i64 0
        v12 = call fn1(v0, v10, v11)  ; v11 = 0
        v13 = iconst.i64 0
        v14 = icmp eq v12, v13  ; v13 = 0
        brif v14, block3, block2

    block2 cold:
        v15 = iconst.i64 2
        call fn2(v0, v15)  ; v15 = 2
        v16 = iconst.i64 0
        return v16  ; v16 = 0

    block3:
        v17 = iconst.i64 0
        v18 = iconst.i64 1
        v19 = iconst.i64 0
        call fn3(v0, v1, v17, v18, v19)  ; v17 = 0, v18 = 1, v19 = 0
        v20 = iconst.i64 0
        call fn4(v0, v1, v20)  ; v20 = 0
        v21 = load.i64 notrap aligned v1+8
        v22 = iconst.i64 1
        call fn5(v0, v1, v22)  ; v22 = 1
        v23 = load.i64 notrap aligned v1+16
        return v23
    }
    ");
}

#[test]
fn a_box_and_an_unbox() {
    // 小さい`int`は即値に、`float`は`eml_rt_alloc_float`で箱にする。`int`の`unbox`は即値を確かめ、`BigInt`の箱をロード
    // する。`float`の`unbox`は箱をロードする。検査モードのコードは、各`box`と`unbox`をその頭で数え、箱のロードをそれぞれ
    // 監査する
    let text = "\
fn f(n.0: int, x.1: float) -> int {
  let a.2: tobj = box n.0
  let b.3: tobj = box x.1
  let m.4: int = unbox a.2
  let y.5: float = unbox b.3
  decref a.2
  decref b.3
  return m.4
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64, f64) -> i64 tail {
        sig0 = (i64, i64, f64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) -> i64 c
        sig3 = (i64, f64) -> i64 c
        sig4 = (i64, i64) c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_alloc_int sig2
        fn3 = %eml_rt_alloc_float sig3
        fn4 = %eml_rt_decref_slow sig4

    block0(v0: i64, v1: i64, v2: f64):
        v3 = load.i64 notrap aligned v0+40
        v4 = load.i32 notrap aligned v3
        v5 = iconst.i32 0
        v6 = icmp ne v4, v5  ; v5 = 0
        v7 = get_stack_pointer.i64 
        v8 = load.i64 notrap aligned v0+48
        v9 = icmp ult v7, v8
        v10 = bor v6, v9
        brif v10, block1, block3

    block1 cold:
        v11 = func_addr.i64 fn0
        v12 = iconst.i64 0
        v13 = call fn1(v0, v11, v12)  ; v12 = 0
        v14 = iconst.i64 0
        v15 = icmp eq v13, v14  ; v14 = 0
        brif v15, block3, block2

    block2 cold:
        v16 = iconst.i64 0
        return v16  ; v16 = 0

    block3:
        v18 = iconst.i64 1
        v19 = ishl.i64 v1, v18  ; v18 = 1
        v20 = iconst.i64 1
        v21 = sshr v19, v20  ; v20 = 1
        v22 = icmp eq v21, v1
        brif v22, block4, block5

    block4:
        v23 = iconst.i64 1
        v24 = ishl.i64 v1, v23  ; v23 = 1
        v25 = iconst.i64 1
        v26 = bor v24, v25  ; v25 = 1
        jump block6(v26)

    block5:
        v27 = call fn2(v0, v1)
        jump block6(v27)

    block6(v17: i64):
        v28 = call fn3(v0, v2)
        v30 = iconst.i64 1
        v31 = band v17, v30  ; v30 = 1
        brif v31, block7, block8

    block7:
        v32 = iconst.i64 1
        v33 = sshr.i64 v17, v32  ; v32 = 1
        jump block9(v33)

    block8:
        v34 = load.i64 notrap aligned v17+8
        jump block9(v34)

    block9(v29: i64):
        v35 = load.f64 notrap aligned v28+8
        v36 = iconst.i64 1
        v37 = band.i64 v17, v36  ; v36 = 1
        brif v37, block13, block14

    block14:
        v38 = load.i32 notrap aligned v17
        v39 = iconst.i32 -2
        v40 = iadd v38, v39  ; v39 = -2
        v41 = iconst.i32 0x7fff_fffd
        v42 = icmp ult v40, v41  ; v41 = 0x7fff_fffd
        brif v42, block10, block11

    block10:
        v43 = iconst.i32 -1
        v44 = iadd.i32 v38, v43  ; v43 = -1
        store notrap aligned v44, v17
        jump block13

    block11:
        brif.i32 v38, block12, block13

    block12 cold:
        call fn4(v0, v17)
        jump block13

    block13:
        v45 = iconst.i64 1
        v46 = band.i64 v28, v45  ; v45 = 1
        brif v46, block18, block19

    block19:
        v47 = load.i32 notrap aligned v28
        v48 = iconst.i32 -2
        v49 = iadd v47, v48  ; v48 = -2
        v50 = iconst.i32 0x7fff_fffd
        v51 = icmp ult v49, v50  ; v50 = 0x7fff_fffd
        brif v51, block15, block16

    block15:
        v52 = iconst.i32 -1
        v53 = iadd.i32 v47, v52  ; v52 = -1
        store notrap aligned v53, v28
        jump block18

    block16:
        brif.i32 v47, block17, block18

    block17 cold:
        call fn4(v0, v28)
        jump block18

    block18:
        return v29
    }
    ");
    insta::assert_snapshot!(twin(text, "f"), @"
    function %f:f(i64, i64, f64) -> i64 tail {
        sig0 = (i64, i64, f64) -> i64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) -> i64 c
        sig4 = (i64, f64) -> i64 c
        sig5 = (i64, i64, i64) c
        sig6 = (i64, i64) c
        sig7 = (i64, i64) c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_audit_count sig2
        fn3 = %eml_rt_alloc_int sig3
        fn4 = %eml_rt_alloc_float sig4
        fn5 = %eml_rt_audit_box sig5
        fn6 = %eml_rt_decref_slow sig6
        fn7 = %eml_rt_audit_decref sig7

    block0(v0: i64, v1: i64, v2: f64):
        v3 = load.i64 notrap aligned v0+40
        v4 = load.i32 notrap aligned v3
        v5 = iconst.i32 0
        v6 = icmp ne v4, v5  ; v5 = 0
        v7 = get_stack_pointer.i64 
        v8 = load.i64 notrap aligned v0+48
        v9 = icmp ult v7, v8
        v10 = bor v6, v9
        brif v10, block1, block3

    block1 cold:
        v11 = func_addr.i64 fn0
        v12 = iconst.i64 0
        v13 = call fn1(v0, v11, v12)  ; v12 = 0
        v14 = iconst.i64 0
        v15 = icmp eq v13, v14  ; v14 = 0
        brif v15, block3, block2

    block2 cold:
        v16 = iconst.i64 2
        call fn2(v0, v16)  ; v16 = 2
        v17 = iconst.i64 0
        return v17  ; v17 = 0

    block3:
        v18 = iconst.i64 0
        call fn2(v0, v18)  ; v18 = 0
        v20 = iconst.i64 1
        v21 = ishl.i64 v1, v20  ; v20 = 1
        v22 = iconst.i64 1
        v23 = sshr v21, v22  ; v22 = 1
        v24 = icmp eq v23, v1
        brif v24, block4, block5

    block4:
        v25 = iconst.i64 1
        v26 = ishl.i64 v1, v25  ; v25 = 1
        v27 = iconst.i64 1
        v28 = bor v26, v27  ; v27 = 1
        jump block6(v28)

    block5:
        v29 = call fn3(v0, v1)
        jump block6(v29)

    block6(v19: i64):
        v30 = iconst.i64 0
        call fn2(v0, v30)  ; v30 = 0
        v31 = call fn4(v0, v2)
        v32 = iconst.i64 1
        call fn2(v0, v32)  ; v32 = 1
        v34 = iconst.i64 1
        v35 = band v19, v34  ; v34 = 1
        brif v35, block7, block8

    block7:
        v36 = iconst.i64 1
        v37 = sshr.i64 v19, v36  ; v36 = 1
        jump block9(v37)

    block8:
        v38 = iconst.i64 0
        call fn5(v0, v19, v38)  ; v38 = 0
        v39 = load.i64 notrap aligned v19+8
        jump block9(v39)

    block9(v33: i64):
        v40 = iconst.i64 1
        call fn2(v0, v40)  ; v40 = 1
        v41 = iconst.i64 1
        call fn5(v0, v31, v41)  ; v41 = 1
        v42 = load.f64 notrap aligned v31+8
        v43 = iconst.i64 1
        v44 = band.i64 v19, v43  ; v43 = 1
        brif v44, block13, block14

    block14:
        call fn7(v0, v19)
        v45 = load.i32 notrap aligned v19
        v46 = iconst.i32 -2
        v47 = iadd v45, v46  ; v46 = -2
        v48 = iconst.i32 0x7fff_fffd
        v49 = icmp ult v47, v48  ; v48 = 0x7fff_fffd
        brif v49, block10, block11

    block10:
        v50 = iconst.i32 -1
        v51 = iadd.i32 v45, v50  ; v50 = -1
        store notrap aligned v51, v19
        jump block13

    block11:
        brif.i32 v45, block12, block13

    block12 cold:
        call fn6(v0, v19)
        jump block13

    block13:
        v52 = iconst.i64 1
        v53 = band.i64 v31, v52  ; v52 = 1
        brif v53, block18, block19

    block19:
        call fn7(v0, v31)
        v54 = load.i32 notrap aligned v31
        v55 = iconst.i32 -2
        v56 = iadd v54, v55  ; v55 = -2
        v57 = iconst.i32 0x7fff_fffd
        v58 = icmp ult v56, v57  ; v57 = 0x7fff_fffd
        brif v58, block15, block16

    block15:
        v59 = iconst.i32 -1
        v60 = iadd.i32 v54, v59  ; v59 = -1
        store notrap aligned v60, v31
        jump block18

    block16:
        brif.i32 v54, block17, block18

    block17 cold:
        call fn6(v0, v31)
        jump block18

    block18:
        return v33
    }
    ");
}

#[test]
fn a_struct_unbox() {
    // 構造体の`unbox`はダミーを確かめ (`is_big_int`)、物体の葉をロードする。`release unboxed`はもう一度ダミーを確かめる。
    // 検査モードのコードは、ダミーの確かめの前にヘッダを、葉の前に形を監査する
    let text = "\
layout Named { Named(obj, float) }
fn main() -> unit {
  return ()
}
fn f(p.2: tobj) -> float {
  let (p.0.4: obj, p.1.5: float) = unbox p.2 Named
  release unboxed p.2 Named #0(p.0.4, _)
  decref p.0.4
  return p.1.5
}
";
    insta::assert_snapshot!(clif(text, "f"), @"
    function %f:f(i64, i64) -> f64 tail {
        gv0 = symbol colocated %k:1
        sig0 = (i64, i64) -> f64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64, i64, i64) c
        sig5 = (i64, i64) c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = %eml_rt_free_block sig3
        fn4 = %eml_rt_release sig4
        fn5 = %eml_rt_decref_slow sig5

    block0(v0: i64, v1: i64):
        v2 = load.i64 notrap aligned v0+40
        v3 = load.i32 notrap aligned v2
        v4 = iconst.i32 0
        v5 = icmp ne v3, v4  ; v4 = 0
        v6 = get_stack_pointer.i64 
        v7 = load.i64 notrap aligned v0+48
        v8 = icmp ult v6, v7
        v9 = bor v5, v8
        brif v9, block1, block3

    block1 cold:
        v10 = func_addr.i64 fn0
        v11 = iconst.i64 0
        v12 = call fn1(v0, v10, v11)  ; v11 = 0
        v13 = iconst.i64 0
        v14 = icmp eq v12, v13  ; v13 = 0
        brif v14, block3, block2

    block2 cold:
        call fn2(v0, v1)
        v15 = f64const 0.0
        return v15  ; v15 = 0.0

    block3:
        v18 = load.i32 notrap aligned v1+4
        v19 = iconst.i32 0x0f00_0000
        v20 = band v18, v19  ; v19 = 0x0f00_0000
        v21 = iconst.i32 0x0100_0000
        v22 = icmp eq v20, v21  ; v21 = 0x0100_0000
        brif v22, block4, block5

    block4 cold:
        v23 = f64const 0.0
        jump block6(v1, v23)  ; v23 = 0.0

    block5:
        v24 = load.i64 notrap aligned v1+8
        v25 = load.f64 notrap aligned v1+16
        jump block6(v24, v25)

    block6(v16: i64, v17: f64):
        v26 = load.i32 notrap aligned v1+4
        v27 = iconst.i32 0x0f00_0000
        v28 = band v26, v27  ; v27 = 0x0f00_0000
        v29 = iconst.i32 0x0100_0000
        v30 = icmp eq v28, v29  ; v29 = 0x0100_0000
        brif v30, block8, block9

    block8 cold:
        jump block7

    block9:
        v31 = load.i32 notrap aligned v1
        v32 = iconst.i32 1
        v33 = icmp eq v31, v32  ; v32 = 1
        brif v33, block10, block11

    block10:
        call fn3(v0, v1)
        jump block7

    block11 cold:
        v34 = symbol_value.i64 gv0
        v35 = iconst.i64 1
        call fn4(v0, v1, v34, v35)  ; v35 = 1
        jump block7

    block7:
        v36 = load.i32 notrap aligned v16
        v37 = iconst.i32 -2
        v38 = iadd v36, v37  ; v37 = -2
        v39 = iconst.i32 0x7fff_fffd
        v40 = icmp ult v38, v39  ; v39 = 0x7fff_fffd
        brif v40, block12, block13

    block12:
        v41 = iconst.i32 -1
        v42 = iadd.i32 v36, v41  ; v41 = -1
        store notrap aligned v42, v16
        jump block15

    block13:
        brif.i32 v36, block14, block15

    block14 cold:
        call fn5(v0, v16)
        jump block15

    block15:
        return v17
    }
    ");
    insta::assert_snapshot!(twin(text, "f"), @"
    function %f:f(i64, i64) -> f64 tail {
        gv0 = symbol colocated %k:1
        sig0 = (i64, i64) -> f64 tail
        sig1 = (i64, i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64) c
        sig5 = (i64, i64, i64, i64, i64) c
        sig6 = (i64, i64, i64) c
        sig7 = (i64, i64, i64) c
        sig8 = (i64, i64) c
        sig9 = (i64, i64) c
        sig10 = (i64, i64, i64, i64) c
        sig11 = (i64, i64) c
        sig12 = (i64, i64) c
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_audit_count sig2
        fn3 = %eml_rt_decref sig3
        fn4 = %eml_rt_audit_header sig4
        fn5 = %eml_rt_audit_data sig5
        fn6 = %eml_rt_audit_field sig6
        fn7 = %eml_rt_audit_raw sig7
        fn8 = %eml_rt_audit_unique sig8
        fn9 = %eml_rt_free_block sig9
        fn10 = %eml_rt_release sig10
        fn11 = %eml_rt_decref_slow sig11
        fn12 = %eml_rt_audit_decref sig12

    block0(v0: i64, v1: i64):
        v2 = load.i64 notrap aligned v0+40
        v3 = load.i32 notrap aligned v2
        v4 = iconst.i32 0
        v5 = icmp ne v3, v4  ; v4 = 0
        v6 = get_stack_pointer.i64 
        v7 = load.i64 notrap aligned v0+48
        v8 = icmp ult v6, v7
        v9 = bor v5, v8
        brif v9, block1, block3

    block1 cold:
        v10 = func_addr.i64 fn0
        v11 = iconst.i64 0
        v12 = call fn1(v0, v10, v11)  ; v11 = 0
        v13 = iconst.i64 0
        v14 = icmp eq v12, v13  ; v13 = 0
        brif v14, block3, block2

    block2 cold:
        v15 = iconst.i64 2
        call fn2(v0, v15)  ; v15 = 2
        call fn3(v0, v1)
        v16 = f64const 0.0
        return v16  ; v16 = 0.0

    block3:
        call fn4(v0, v1)
        v19 = load.i32 notrap aligned v1+4
        v20 = iconst.i32 0x0f00_0000
        v21 = band v19, v20  ; v20 = 0x0f00_0000
        v22 = iconst.i32 0x0100_0000
        v23 = icmp eq v21, v22  ; v22 = 0x0100_0000
        brif v23, block4, block5

    block4 cold:
        v24 = f64const 0.0
        jump block6(v1, v24)  ; v24 = 0.0

    block5:
        v25 = iconst.i64 0
        v26 = iconst.i64 1
        v27 = iconst.i64 3
        call fn5(v0, v1, v25, v26, v27)  ; v25 = 0, v26 = 1, v27 = 3
        v28 = iconst.i64 0
        call fn6(v0, v1, v28)  ; v28 = 0
        v29 = load.i64 notrap aligned v1+8
        v30 = iconst.i64 1
        call fn7(v0, v1, v30)  ; v30 = 1
        v31 = load.f64 notrap aligned v1+16
        jump block6(v29, v31)

    block6(v17: i64, v18: f64):
        call fn4(v0, v1)
        v32 = load.i32 notrap aligned v1+4
        v33 = iconst.i32 0x0f00_0000
        v34 = band v32, v33  ; v33 = 0x0f00_0000
        v35 = iconst.i32 0x0100_0000
        v36 = icmp eq v34, v35  ; v35 = 0x0100_0000
        brif v36, block8, block9

    block8 cold:
        jump block7

    block9:
        v37 = iconst.i64 0
        v38 = iconst.i64 1
        v39 = iconst.i64 2
        call fn5(v0, v1, v37, v38, v39)  ; v37 = 0, v38 = 1, v39 = 2
        call fn8(v0, v1)
        v40 = load.i32 notrap aligned v1
        v41 = iconst.i32 1
        v42 = icmp eq v40, v41  ; v41 = 1
        brif v42, block10, block11

    block10:
        call fn9(v0, v1)
        jump block7

    block11 cold:
        v43 = symbol_value.i64 gv0
        v44 = iconst.i64 1
        call fn10(v0, v1, v43, v44)  ; v44 = 1
        jump block7

    block7:
        call fn12(v0, v17)
        v45 = load.i32 notrap aligned v17
        v46 = iconst.i32 -2
        v47 = iadd v45, v46  ; v46 = -2
        v48 = iconst.i32 0x7fff_fffd
        v49 = icmp ult v47, v48  ; v48 = 0x7fff_fffd
        brif v49, block12, block13

    block12:
        v50 = iconst.i32 -1
        v51 = iadd.i32 v45, v50  ; v50 = -1
        store notrap aligned v51, v17
        jump block15

    block13:
        brif.i32 v45, block14, block15

    block14 cold:
        call fn11(v0, v17)
        jump block15

    block15:
        return v18
    }
    ");
}

#[test]
fn a_closure_entry_and_a_pap_adapter() {
    // クロージャのエントリ`c2:k`は捕獲をロードし、箱を手放し (`release_box`)、`k`を末尾で呼ぶ。PAPのアダプタ`eml$pap$1`は
    // 呼ぶ値と引数をロードし、箱を手放して、呼ぶ値のコードを末尾で間接に呼ぶ。検査モードのコードは、どのロードも一意かの
    // 確かめも監査し、共有の腕のインラインの数の操作の前に数の監査を置き、呼ぶ値のコードを呼び出しの署名で監査する
    let text = "\
fn k(s.0: obj, n.1: int, a.2: tobj, b.3: tobj) -> tobj {
  decref s.0
  decref a.2
  return b.3
}
fn f(s.0: obj, n.1: int) -> tobj {
  let c.2: tobj = closure k(s.0, n.1)
  return c.2
}
";
    insta::assert_snapshot!(clif(text, "c2:k"), @"
    function %c2:k(i64, i64, i64, i64) -> i64 tail {
        sig0 = (i64, i64) -> i64 c
        sig1 = (i64, i64) c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64, i64, i64, i64) -> i64 tail
        fn0 = %eml_rt_is_last sig0
        fn1 = %eml_rt_free_block sig1
        fn2 = %eml_rt_dup_slow sig2
        fn3 = %eml_rt_decref_slow sig3
        fn4 = colocated %f:k sig4

    block0(v0: i64, v1: i64, v2: i64, v3: i64):
        v4 = load.i64 notrap aligned v1+16
        v5 = load.i64 notrap aligned v1+24
        v6 = load.i32 notrap aligned v1
        v7 = iconst.i32 1
        v8 = icmp eq v6, v7  ; v7 = 1
        brif v8, block1, block2

    block2 cold:
        v9 = call fn0(v0, v1)
        brif v9, block1, block3

    block1:
        call fn1(v0, v1)
        jump block4

    block3:
        v10 = load.i32 notrap aligned v4
        v11 = iconst.i32 -1
        v12 = iadd v10, v11  ; v11 = -1
        v13 = iconst.i32 0x7fff_fffe
        v14 = icmp ult v12, v13  ; v13 = 0x7fff_fffe
        brif v14, block5, block6

    block5:
        v15 = iconst.i32 1
        v16 = iadd.i32 v10, v15  ; v15 = 1
        store notrap aligned v16, v4
        jump block8

    block6:
        brif.i32 v10, block7, block8

    block7 cold:
        call fn2(v0, v4)
        jump block8

    block8:
        v17 = load.i32 notrap aligned v1
        v18 = iconst.i32 -2
        v19 = iadd v17, v18  ; v18 = -2
        v20 = iconst.i32 0x7fff_fffd
        v21 = icmp ult v19, v20  ; v20 = 0x7fff_fffd
        brif v21, block9, block10

    block9:
        v22 = iconst.i32 -1
        v23 = iadd.i32 v17, v22  ; v22 = -1
        store notrap aligned v23, v1
        jump block12

    block10:
        brif.i32 v17, block11, block12

    block11 cold:
        call fn3(v0, v1)
        jump block12

    block12:
        jump block4

    block4:
        return_call fn4(v0, v4, v5, v2, v3)
    }
    ");
    insta::assert_snapshot!(twin(text, "c2:k"), @"
    function %c2:k(i64, i64, i64, i64) -> i64 tail {
        sig0 = (i64, i64, i64) c
        sig1 = (i64, i64, i64) c
        sig2 = (i64, i64) -> i64 c
        sig3 = (i64, i64) c
        sig4 = (i64, i64) c
        sig5 = (i64, i64) c
        sig6 = (i64, i64) c
        sig7 = (i64, i64) c
        sig8 = (i64, i64) c
        sig9 = (i64, i64, i64, i64, i64) -> i64 tail
        fn0 = %eml_rt_audit_field sig0
        fn1 = %eml_rt_audit_raw sig1
        fn2 = %eml_rt_is_last sig2
        fn3 = %eml_rt_free_block sig3
        fn4 = %eml_rt_dup_slow sig4
        fn5 = %eml_rt_decref_slow sig5
        fn6 = %eml_rt_audit_dup sig6
        fn7 = %eml_rt_audit_decref sig7
        fn8 = %eml_rt_audit_unique sig8
        fn9 = colocated %f:k sig9

    block0(v0: i64, v1: i64, v2: i64, v3: i64):
        v4 = iconst.i64 1
        call fn0(v0, v1, v4)  ; v4 = 1
        v5 = load.i64 notrap aligned v1+16
        v6 = iconst.i64 2
        call fn1(v0, v1, v6)  ; v6 = 2
        v7 = load.i64 notrap aligned v1+24
        call fn8(v0, v1)
        v8 = load.i32 notrap aligned v1
        v9 = iconst.i32 1
        v10 = icmp eq v8, v9  ; v9 = 1
        brif v10, block1, block2

    block2 cold:
        v11 = call fn2(v0, v1)
        brif v11, block1, block3

    block1:
        call fn3(v0, v1)
        jump block4

    block3:
        call fn6(v0, v5)
        v12 = load.i32 notrap aligned v5
        v13 = iconst.i32 -1
        v14 = iadd v12, v13  ; v13 = -1
        v15 = iconst.i32 0x7fff_fffe
        v16 = icmp ult v14, v15  ; v15 = 0x7fff_fffe
        brif v16, block5, block6

    block5:
        v17 = iconst.i32 1
        v18 = iadd.i32 v12, v17  ; v17 = 1
        store notrap aligned v18, v5
        jump block8

    block6:
        brif.i32 v12, block7, block8

    block7 cold:
        call fn4(v0, v5)
        jump block8

    block8:
        call fn7(v0, v1)
        v19 = load.i32 notrap aligned v1
        v20 = iconst.i32 -2
        v21 = iadd v19, v20  ; v20 = -2
        v22 = iconst.i32 0x7fff_fffd
        v23 = icmp ult v21, v22  ; v22 = 0x7fff_fffd
        brif v23, block9, block10

    block9:
        v24 = iconst.i32 -1
        v25 = iadd.i32 v19, v24  ; v24 = -1
        store notrap aligned v25, v1
        jump block12

    block10:
        brif.i32 v19, block11, block12

    block11 cold:
        call fn5(v0, v1)
        jump block12

    block12:
        jump block4

    block4:
        return_call fn9(v0, v5, v7, v2, v3)
    }
    ");
    insta::assert_snapshot!(clif(text, "eml$pap$1"), @"
    function %eml$pap$1(i64, i64, i64) -> i64 tail {
        sig0 = (i64, i64) -> i64 c
        sig1 = (i64, i64) c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64, i64, i64) -> i64 tail
        fn0 = %eml_rt_is_last sig0
        fn1 = %eml_rt_free_block sig1
        fn2 = %eml_rt_dup_slow sig2
        fn3 = %eml_rt_decref_slow sig3

    block0(v0: i64, v1: i64, v2: i64):
        v3 = load.i64 notrap aligned v1+16
        v4 = load.i64 notrap aligned v1+24
        v5 = load.i32 notrap aligned v1
        v6 = iconst.i32 1
        v7 = icmp eq v5, v6  ; v6 = 1
        brif v7, block1, block2

    block2 cold:
        v8 = call fn0(v0, v1)
        brif v8, block1, block3

    block1:
        call fn1(v0, v1)
        jump block4

    block3:
        v9 = load.i32 notrap aligned v3
        v10 = iconst.i32 -1
        v11 = iadd v9, v10  ; v10 = -1
        v12 = iconst.i32 0x7fff_fffe
        v13 = icmp ult v11, v12  ; v12 = 0x7fff_fffe
        brif v13, block5, block6

    block5:
        v14 = iconst.i32 1
        v15 = iadd.i32 v9, v14  ; v14 = 1
        store notrap aligned v15, v3
        jump block8

    block6:
        brif.i32 v9, block7, block8

    block7 cold:
        call fn2(v0, v3)
        jump block8

    block8:
        v16 = iconst.i64 1
        v17 = band.i64 v4, v16  ; v16 = 1
        brif v17, block12, block13

    block13:
        v18 = load.i32 notrap aligned v4
        v19 = iconst.i32 -1
        v20 = iadd v18, v19  ; v19 = -1
        v21 = iconst.i32 0x7fff_fffe
        v22 = icmp ult v20, v21  ; v21 = 0x7fff_fffe
        brif v22, block9, block10

    block9:
        v23 = iconst.i32 1
        v24 = iadd.i32 v18, v23  ; v23 = 1
        store notrap aligned v24, v4
        jump block12

    block10:
        brif.i32 v18, block11, block12

    block11 cold:
        call fn2(v0, v4)
        jump block12

    block12:
        v25 = load.i32 notrap aligned v1
        v26 = iconst.i32 -2
        v27 = iadd v25, v26  ; v26 = -2
        v28 = iconst.i32 0x7fff_fffd
        v29 = icmp ult v27, v28  ; v28 = 0x7fff_fffd
        brif v29, block14, block15

    block14:
        v30 = iconst.i32 -1
        v31 = iadd.i32 v25, v30  ; v30 = -1
        store notrap aligned v31, v1
        jump block17

    block15:
        brif.i32 v25, block16, block17

    block16 cold:
        call fn3(v0, v1)
        jump block17

    block17:
        jump block4

    block4:
        v32 = load.i32 notrap aligned v3+4
        v33 = iconst.i32 16
        v34 = ushr v32, v33  ; v33 = 16
        v35 = iconst.i32 255
        v36 = band v34, v35  ; v35 = 255
        v37 = iconst.i32 255
        v38 = icmp eq v36, v37  ; v37 = 255
        v39 = iconst.i64 8
        v40 = iconst.i64 16
        v41 = select v38, v40, v39  ; v40 = 16, v39 = 8
        v42 = iadd.i64 v3, v41
        v43 = load.i64 notrap aligned v42
        v44 = iconst.i64 1
        v45 = sshr v43, v44  ; v44 = 1
        return_call_indirect sig4, v45(v0, v3, v4, v2)
    }
    ");
    insta::assert_snapshot!(twin(text, "eml$pap$1"), @"
    function %eml$pap$1(i64, i64, i64) -> i64 tail {
        sig0 = (i64, i64, i64) c
        sig1 = (i64, i64) -> i64 c
        sig2 = (i64, i64) c
        sig3 = (i64, i64) c
        sig4 = (i64, i64) c
        sig5 = (i64, i64) c
        sig6 = (i64, i64) c
        sig7 = (i64, i64) c
        sig8 = (i64, i64, i64) c
        sig9 = (i64, i64, i64, i64) -> i64 tail
        fn0 = %eml_rt_audit_field sig0
        fn1 = %eml_rt_is_last sig1
        fn2 = %eml_rt_free_block sig2
        fn3 = %eml_rt_dup_slow sig3
        fn4 = %eml_rt_decref_slow sig4
        fn5 = %eml_rt_audit_dup sig5
        fn6 = %eml_rt_audit_decref sig6
        fn7 = %eml_rt_audit_unique sig7
        fn8 = %eml_rt_audit_code sig8

    block0(v0: i64, v1: i64, v2: i64):
        v3 = iconst.i64 1
        call fn0(v0, v1, v3)  ; v3 = 1
        v4 = load.i64 notrap aligned v1+16
        v5 = iconst.i64 2
        call fn0(v0, v1, v5)  ; v5 = 2
        v6 = load.i64 notrap aligned v1+24
        call fn7(v0, v1)
        v7 = load.i32 notrap aligned v1
        v8 = iconst.i32 1
        v9 = icmp eq v7, v8  ; v8 = 1
        brif v9, block1, block2

    block2 cold:
        v10 = call fn1(v0, v1)
        brif v10, block1, block3

    block1:
        call fn2(v0, v1)
        jump block4

    block3:
        call fn5(v0, v4)
        v11 = load.i32 notrap aligned v4
        v12 = iconst.i32 -1
        v13 = iadd v11, v12  ; v12 = -1
        v14 = iconst.i32 0x7fff_fffe
        v15 = icmp ult v13, v14  ; v14 = 0x7fff_fffe
        brif v15, block5, block6

    block5:
        v16 = iconst.i32 1
        v17 = iadd.i32 v11, v16  ; v16 = 1
        store notrap aligned v17, v4
        jump block8

    block6:
        brif.i32 v11, block7, block8

    block7 cold:
        call fn3(v0, v4)
        jump block8

    block8:
        v18 = iconst.i64 1
        v19 = band.i64 v6, v18  ; v18 = 1
        brif v19, block12, block13

    block13:
        call fn5(v0, v6)
        v20 = load.i32 notrap aligned v6
        v21 = iconst.i32 -1
        v22 = iadd v20, v21  ; v21 = -1
        v23 = iconst.i32 0x7fff_fffe
        v24 = icmp ult v22, v23  ; v23 = 0x7fff_fffe
        brif v24, block9, block10

    block9:
        v25 = iconst.i32 1
        v26 = iadd.i32 v20, v25  ; v25 = 1
        store notrap aligned v26, v6
        jump block12

    block10:
        brif.i32 v20, block11, block12

    block11 cold:
        call fn3(v0, v6)
        jump block12

    block12:
        call fn6(v0, v1)
        v27 = load.i32 notrap aligned v1
        v28 = iconst.i32 -2
        v29 = iadd v27, v28  ; v28 = -2
        v30 = iconst.i32 0x7fff_fffd
        v31 = icmp ult v29, v30  ; v30 = 0x7fff_fffd
        brif v31, block14, block15

    block14:
        v32 = iconst.i32 -1
        v33 = iadd.i32 v27, v32  ; v32 = -1
        store notrap aligned v33, v1
        jump block17

    block15:
        brif.i32 v27, block16, block17

    block16 cold:
        call fn4(v0, v1)
        jump block17

    block17:
        jump block4

    block4:
        v34 = iconst.i64 2
        call fn8(v0, v4, v34)  ; v34 = 2
        v35 = load.i32 notrap aligned v4+4
        v36 = iconst.i32 16
        v37 = ushr v35, v36  ; v36 = 16
        v38 = iconst.i32 255
        v39 = band v37, v38  ; v38 = 255
        v40 = iconst.i32 255
        v41 = icmp eq v39, v40  ; v40 = 255
        v42 = iconst.i64 8
        v43 = iconst.i64 16
        v44 = select v41, v43, v42  ; v43 = 16, v42 = 8
        v45 = iadd.i64 v4, v44
        v46 = load.i64 notrap aligned v45
        v47 = iconst.i64 1
        v48 = sshr v46, v47  ; v47 = 1
        return_call_indirect sig9, v48(v0, v4, v6, v2)
    }
    ");
}

/// 上の固定のうち、関数がヒープの物体をインラインで読み書きするものの、検査モードの双子。どの固定も検査モードのCLIFで、
/// `twin`がそれを、速いCLIFに監査を足したものだと確かめる (docs/implementation/testing.mdの「`eml_native` の検証」)。
mod checked {
    use super::RELEASES;
    use crate::common::native::twin;

    #[test]
    fn inline_dup_and_decref() {
        let text = "\
fn f(x.0: obj, borrowed y.1: tobj) -> tobj {
  decref x.0
  dup y.1
  return y.1
}
";

        insta::assert_snapshot!(twin(text, "f"), @"
        function %f:f(i64, i64, i64) -> i64 tail {
            sig0 = (i64, i64, i64) -> i64 tail
            sig1 = (i64, i64, i64) -> i64 c
            sig2 = (i64, i64) c
            sig3 = (i64, i64) c
            sig4 = (i64, i64) c
            sig5 = (i64, i64) c
            sig6 = (i64, i64) c
            sig7 = (i64, i64) c
            fn0 = colocated %f:f sig0
            fn1 = %eml_nrt_safepoint sig1
            fn2 = %eml_rt_audit_count sig2
            fn3 = %eml_rt_decref sig3
            fn4 = %eml_rt_decref_slow sig4
            fn5 = %eml_rt_audit_decref sig5
            fn6 = %eml_rt_dup_slow sig6
            fn7 = %eml_rt_audit_dup sig7

        block0(v0: i64, v1: i64, v2: i64):
            v3 = load.i64 notrap aligned v0+40
            v4 = load.i32 notrap aligned v3
            v5 = iconst.i32 0
            v6 = icmp ne v4, v5  ; v5 = 0
            v7 = get_stack_pointer.i64 
            v8 = load.i64 notrap aligned v0+48
            v9 = icmp ult v7, v8
            v10 = bor v6, v9
            brif v10, block1, block3

        block1 cold:
            v11 = func_addr.i64 fn0
            v12 = iconst.i64 0
            v13 = call fn1(v0, v11, v12)  ; v12 = 0
            v14 = iconst.i64 0
            v15 = icmp eq v13, v14  ; v14 = 0
            brif v15, block3, block2

        block2 cold:
            v16 = iconst.i64 2
            call fn2(v0, v16)  ; v16 = 2
            call fn3(v0, v1)
            v17 = iconst.i64 1
            return v17  ; v17 = 1

        block3:
            call fn5(v0, v1)
            v18 = load.i32 notrap aligned v1
            v19 = iconst.i32 -2
            v20 = iadd v18, v19  ; v19 = -2
            v21 = iconst.i32 0x7fff_fffd
            v22 = icmp ult v20, v21  ; v21 = 0x7fff_fffd
            brif v22, block4, block5

        block4:
            v23 = iconst.i32 -1
            v24 = iadd.i32 v18, v23  ; v23 = -1
            store notrap aligned v24, v1
            jump block7

        block5:
            brif.i32 v18, block6, block7

        block6 cold:
            call fn4(v0, v1)
            jump block7

        block7:
            v25 = iconst.i64 1
            v26 = band.i64 v2, v25  ; v25 = 1
            brif v26, block11, block12

        block12:
            call fn7(v0, v2)
            v27 = load.i32 notrap aligned v2
            v28 = iconst.i32 -1
            v29 = iadd v27, v28  ; v28 = -1
            v30 = iconst.i32 0x7fff_fffe
            v31 = icmp ult v29, v30  ; v30 = 0x7fff_fffe
            brif v31, block8, block9

        block8:
            v32 = iconst.i32 1
            v33 = iadd.i32 v27, v32  ; v32 = 1
            store notrap aligned v33, v2
            jump block11

        block9:
            brif.i32 v27, block10, block11

        block10 cold:
            call fn6(v0, v2)
            jump block11

        block11:
            return v2
        }
        ");
    }

    #[test]
    fn a_tag_switch() {
        let text = "\
layout Opt { None, Some(int, float) }
fn f(borrowed o.0: tobj) -> int {
  switch o.0 Opt { #0 -> b1, #1(n.1: int, x.2: float) -> b2 }
b1:
  return 0
b2:
  return n.1
}
";

        insta::assert_snapshot!(twin(text, "f"), @"
        function %f:f(i64, i64) -> i64 tail {
            sig0 = (i64, i64) -> i64 tail
            sig1 = (i64, i64, i64) -> i64 c
            sig2 = (i64, i64) c
            sig3 = (i64, i64) c
            sig4 = (i64, i64, i64, i64, i64) c
            sig5 = (i64, i64, i64) c
            fn0 = colocated %f:f sig0
            fn1 = %eml_nrt_safepoint sig1
            fn2 = %eml_rt_audit_count sig2
            fn3 = %eml_rt_audit_header sig3
            fn4 = %eml_rt_audit_data sig4
            fn5 = %eml_rt_audit_raw sig5

        block0(v0: i64, v1: i64):
            v2 = load.i64 notrap aligned v0+40
            v3 = load.i32 notrap aligned v2
            v4 = iconst.i32 0
            v5 = icmp ne v3, v4  ; v4 = 0
            v6 = get_stack_pointer.i64 
            v7 = load.i64 notrap aligned v0+48
            v8 = icmp ult v6, v7
            v9 = bor v5, v8
            brif v9, block1, block3

        block1 cold:
            v10 = func_addr.i64 fn0
            v11 = iconst.i64 0
            v12 = call fn1(v0, v10, v11)  ; v11 = 0
            v13 = iconst.i64 0
            v14 = icmp eq v12, v13  ; v13 = 0
            brif v14, block3, block2

        block2 cold:
            v15 = iconst.i64 2
            call fn2(v0, v15)  ; v15 = 2
            v16 = iconst.i64 0
            return v16  ; v16 = 0

        block3:
            v18 = iconst.i64 1
            v19 = band.i64 v1, v18  ; v18 = 1
            brif v19, block7, block8

        block7:
            v20 = iconst.i64 1
            v21 = sshr.i64 v1, v20  ; v20 = 1
            v22 = ireduce.i32 v21
            jump block9(v22)

        block8:
            call fn3(v0, v1)
            v23 = load.i32 notrap aligned v1+4
            v24 = iconst.i32 0xffff
            v25 = band v23, v24  ; v24 = 0xffff
            jump block9(v25)

        block9(v17: i32):
            br_table v17, block6, [block4, block5]

        block6 cold:
            trap user1

        block4:
            v26 = iconst.i64 0
            return v26  ; v26 = 0

        block5:
            v27 = iconst.i64 1
            v28 = iconst.i64 0
            v29 = iconst.i64 1
            call fn4(v0, v1, v27, v28, v29)  ; v27 = 1, v28 = 0, v29 = 1
            v30 = iconst.i64 0
            call fn5(v0, v1, v30)  ; v30 = 0
            v31 = load.i64 notrap aligned v1+8
            v32 = iconst.i64 1
            call fn5(v0, v1, v32)  ; v32 = 1
            v33 = load.f64 notrap aligned v1+16
            return v31
        }
        ");
    }

    #[test]
    fn an_apply_site() {
        let text = "\
fn f(s.0: obj, c.1: tobj, x.2: tobj) -> obj {
  let r.3: tobj = apply c.1(x.2)
  decref r.3
  return s.0
}
";

        insta::assert_snapshot!(twin(text, "f"), @"
        function %f:f(i64, i64, i64, i64) -> i64 tail {
            sig0 = (i64, i64, i64, i64) -> i64 tail
            sig1 = (i64, i64, i64) -> i64 c
            sig2 = (i64, i64) c
            sig3 = (i64, i64) c
            sig4 = (i64, i64, i64) c
            sig5 = (i64, i64, i64) -> i64 tail
            sig6 = (i64, i64, i64) -> i64 tail
            sig7 = (i64, i64) c
            sig8 = (i64, i64) c
            fn0 = colocated %f:f sig0
            fn1 = %eml_nrt_safepoint sig1
            fn2 = %eml_rt_audit_count sig2
            fn3 = %eml_rt_decref sig3
            fn4 = %eml_rt_audit_code sig4
            fn5 = colocated %eml$apply$1 sig6
            fn6 = %eml_rt_decref_slow sig7
            fn7 = %eml_rt_audit_decref sig8

        block0(v0: i64, v1: i64, v2: i64, v3: i64):
            v4 = load.i64 notrap aligned v0+40
            v5 = load.i32 notrap aligned v4
            v6 = iconst.i32 0
            v7 = icmp ne v5, v6  ; v6 = 0
            v8 = get_stack_pointer.i64 
            v9 = load.i64 notrap aligned v0+48
            v10 = icmp ult v8, v9
            v11 = bor v7, v10
            brif v11, block1, block3

        block1 cold:
            v12 = func_addr.i64 fn0
            v13 = iconst.i64 0
            v14 = call fn1(v0, v12, v13)  ; v13 = 0
            v15 = iconst.i64 0
            v16 = icmp eq v14, v15  ; v15 = 0
            brif v16, block3, block2

        block2 cold:
            v17 = iconst.i64 2
            call fn2(v0, v17)  ; v17 = 2
            call fn3(v0, v1)
            call fn3(v0, v2)
            call fn3(v0, v3)
            v18 = iconst.i64 1
            return v18  ; v18 = 1

        block3:
            v19 = iconst.i64 -1
            call fn4(v0, v2, v19)  ; v19 = -1
            v20 = load.i32 notrap aligned v2+4
            v21 = iconst.i32 0xffff
            v22 = band v20, v21  ; v21 = 0xffff
            v23 = iconst.i32 1
            v24 = icmp eq v22, v23  ; v23 = 1
            brif v24, block4, block5

        block4:
            v26 = iconst.i32 16
            v27 = ushr.i32 v20, v26  ; v26 = 16
            v28 = iconst.i32 255
            v29 = band v27, v28  ; v28 = 255
            v30 = iconst.i32 255
            v31 = icmp eq v29, v30  ; v30 = 255
            v32 = iconst.i64 8
            v33 = iconst.i64 16
            v34 = select v31, v33, v32  ; v33 = 16, v32 = 8
            v35 = iadd.i64 v2, v34
            v36 = load.i64 notrap aligned v35
            v37 = iconst.i64 1
            v38 = sshr v36, v37  ; v37 = 1
            v39 = call_indirect sig5, v38(v0, v2, v3)
            jump block6(v39)

        block5:
            v40 = call fn5(v0, v2, v3)
            jump block6(v40)

        block6(v25: i64):
            v41 = load.i8 notrap aligned v0+24
            brif v41, block7, block8

        block7 cold:
            v42 = iconst.i64 2
            call fn2(v0, v42)  ; v42 = 2
            call fn3(v0, v1)
            v43 = iconst.i64 1
            return v43  ; v43 = 1

        block8:
            v44 = iconst.i64 1
            v45 = band.i64 v25, v44  ; v44 = 1
            brif v45, block12, block13

        block13:
            call fn7(v0, v25)
            v46 = load.i32 notrap aligned v25
            v47 = iconst.i32 -2
            v48 = iadd v46, v47  ; v47 = -2
            v49 = iconst.i32 0x7fff_fffd
            v50 = icmp ult v48, v49  ; v49 = 0x7fff_fffd
            brif v50, block9, block10

        block9:
            v51 = iconst.i32 -1
            v52 = iadd.i32 v46, v51  ; v51 = -1
            store notrap aligned v52, v25
            jump block12

        block10:
            brif.i32 v46, block11, block12

        block11 cold:
            call fn6(v0, v25)
            jump block12

        block12:
            return v1
        }
        ");
    }

    #[test]
    fn an_over_application() {
        let text = "\
fn f(x.0: tobj) -> tobj {
  return x.0
}
fn g(a.0: tobj, b.1: tobj) -> tobj {
  tail apply &f(a.0, b.1)
}
";

        insta::assert_snapshot!(twin(text, "eml$apply$2"), @"
        function %eml$apply$2(i64, i64, i64, i64) -> i64 tail {
            ss0 = explicit_slot 16, align = 8
            sig0 = (i64, i64, i64) c
            sig1 = (i64, i64, i64, i64) -> i64 tail
            sig2 = (i64, i64, i64, i64) -> i64 c
            sig3 = (i64, i64, i64, i64) -> i64 tail
            sig4 = (i64, i64, i64) -> i64 c
            sig5 = (i64, i64, i64) -> i64 tail
            sig6 = (i64, i64) c
            sig7 = (i64, i64) c
            sig8 = (i64, i64, i64) -> i64 tail
            fn0 = %eml_rt_audit_code sig0
            fn1 = %eml_nrt_pap sig2
            fn2 = colocated %eml$apply$2 sig3
            fn3 = %eml_nrt_safepoint sig4
            fn4 = colocated %eml$apply$1 sig5
            fn5 = %eml_rt_audit_count sig6
            fn6 = %eml_rt_decref sig7
            fn7 = colocated %eml$apply$1 sig8

        block0(v0: i64, v1: i64, v2: i64, v3: i64):
            v4 = iconst.i64 -1
            call fn0(v0, v1, v4)  ; v4 = -1
            v5 = load.i32 notrap aligned v1+4
            v6 = iconst.i32 0xffff
            v7 = band v5, v6  ; v6 = 0xffff
            v8 = iconst.i32 2
            v9 = icmp eq v7, v8  ; v8 = 2
            brif v9, block1, block2

        block1:
            v10 = iconst.i32 16
            v11 = ushr.i32 v5, v10  ; v10 = 16
            v12 = iconst.i32 255
            v13 = band v11, v12  ; v12 = 255
            v14 = iconst.i32 255
            v15 = icmp eq v13, v14  ; v14 = 255
            v16 = iconst.i64 8
            v17 = iconst.i64 16
            v18 = select v15, v17, v16  ; v17 = 16, v16 = 8
            v19 = iadd.i64 v1, v18
            v20 = load.i64 notrap aligned v19
            v21 = iconst.i64 1
            v22 = sshr v20, v21  ; v21 = 1
            return_call_indirect sig1, v22(v0, v1, v2, v3)

        block2:
            v23 = iconst.i32 2
            v24 = icmp.i32 ugt v7, v23  ; v23 = 2
            brif v24, block3, block4

        block3:
            v25 = stack_addr.i64 ss0
            store.i64 notrap v2, v25
            v26 = stack_addr.i64 ss0+8
            store.i64 notrap v3, v26
            v27 = stack_addr.i64 ss0
            v28 = iconst.i64 2
            v29 = call fn1(v0, v1, v27, v28)  ; v28 = 2
            return v29

        block4:
            v30 = get_stack_pointer.i64 
            v31 = load.i64 notrap aligned v0+48
            v32 = icmp ult v30, v31
            brif v32, block5, block6

        block5 cold:
            v33 = func_addr.i64 fn2
            v34 = iconst.i64 0
            v35 = call fn3(v0, v33, v34)  ; v34 = 0
            v36 = iconst.i64 0
            v37 = icmp eq v35, v36  ; v36 = 0
            brif v37, block6, block7

        block7 cold:
            trap user1

        block6:
            v38 = call fn4(v0, v1, v2)
            v39 = load.i8 notrap aligned v0+24
            brif v39, block8, block9

        block8 cold:
            v40 = iconst.i64 2
            call fn5(v0, v40)  ; v40 = 2
            call fn6(v0, v3)
            v41 = iconst.i64 1
            return v41  ; v41 = 1

        block9:
            v42 = load.i64 notrap aligned v0
            v43 = iconst.i64 1
            v44 = icmp ne v42, v43  ; v43 = 1
            brif v44, block10, block11

        block10 cold:
            trap user1

        block11:
            return_call fn7(v0, v38, v3)
        }
        ");
    }

    #[test]
    fn a_landing_point() {
        let text = "\
effect Land { land(unit) -> tobj }
yields fn g(u.0: tobj) -> tobj {
  return u.0
}
yields fn prompt$(ev.0: ev lin, x.1: tobj) -> tobj {
  let u.2: unit = drop ev.0
  return x.1
}
fn f(w.4: evv, s.1: obj) -> obj {
  let ev.0: ev lin = evidence Land((), w.4) [&land$entry]
  let x.2: tobj = yields apply &g(()) prompt ev.0
  let r.3: tobj = call prompt$(ev.0, x.2)
  decref r.3
  return s.1
}
internal clause Land.land fn land$entry(e.0: cev, u.1: unit) -> tobj {
  decref e.0
  return ()
}
";

        insta::assert_snapshot!(twin(text, "f"), @"
        function %f:f(i64, i64, i64) -> i64 tail {
            gv0 = symbol colocated %v:land$entry
            gv1 = symbol colocated %e:Land
            gv2 = symbol colocated %v:g
            sig0 = (i64, i64, i64) -> i64 tail
            sig1 = (i64, i64, i64) -> i64 c
            sig2 = (i64, i64) c
            sig3 = (i64, i64) c
            sig4 = (i64, i64, i64, i64, i64) -> i64 c
            sig5 = (i64, i64, i64, i64) c
            sig6 = (i64, i64, i64) c
            sig7 = (i64, i64, i64) -> i64 tail
            sig8 = (i64, i64, i64) -> i64 tail
            sig9 = (i64) -> i64 c
            sig10 = (i64, i64) -> i64 c
            sig11 = (i64, i64, i64) -> i64 tail
            sig12 = (i64, i64) c
            sig13 = (i64, i64) c
            fn0 = colocated %f:f sig0
            fn1 = %eml_nrt_safepoint sig1
            fn2 = %eml_rt_audit_count sig2
            fn3 = %eml_rt_decref sig3
            fn4 = %eml_rt_ev_new sig4
            fn5 = %eml_rt_ev_set_clause sig5
            fn6 = %eml_rt_audit_code sig6
            fn7 = colocated %eml$apply$1 sig8
            fn8 = %eml_nrt_landed sig9
            fn9 = %eml_rt_ev_last_state sig10
            fn10 = colocated %f:prompt$ sig11
            fn11 = %eml_rt_decref_slow sig12
            fn12 = %eml_rt_audit_decref sig13

        block0(v0: i64, v1: i64, v2: i64):
            v3 = load.i64 notrap aligned v0+40
            v4 = load.i32 notrap aligned v3
            v5 = iconst.i32 0
            v6 = icmp ne v4, v5  ; v5 = 0
            v7 = get_stack_pointer.i64 
            v8 = load.i64 notrap aligned v0+48
            v9 = icmp ult v7, v8
            v10 = bor v6, v9
            brif v10, block1, block3

        block1 cold:
            v11 = func_addr.i64 fn0
            v12 = iconst.i64 0
            v13 = call fn1(v0, v11, v12)  ; v12 = 0
            v14 = iconst.i64 0
            v15 = icmp eq v13, v14  ; v14 = 0
            brif v15, block3, block2

        block2 cold:
            v16 = iconst.i64 2
            call fn2(v0, v16)  ; v16 = 2
            call fn3(v0, v2)
            call fn3(v0, v1)
            v17 = iconst.i64 1
            return v17  ; v17 = 1

        block3:
            v18 = symbol_value.i64 gv0
            v19 = load.i64 notrap aligned v18
            v20 = iconst.i64 1
            v21 = symbol_value.i64 gv1
            v22 = load.i64 notrap aligned v21
            v23 = iconst.i64 1
            v24 = call fn4(v0, v22, v23, v20, v1)  ; v23 = 1, v20 = 1
            v25 = iconst.i64 0
            call fn5(v0, v24, v25, v19)  ; v25 = 0
            v26 = symbol_value.i64 gv2
            v27 = load.i64 notrap aligned v26
            v28 = iconst.i64 1
            v29 = iconst.i64 -1
            call fn6(v0, v27, v29)  ; v29 = -1
            v30 = load.i32 notrap aligned v27+4
            v31 = iconst.i32 0xffff
            v32 = band v30, v31  ; v31 = 0xffff
            v33 = iconst.i32 1
            v34 = icmp eq v32, v33  ; v33 = 1
            brif v34, block4, block5

        block4:
            v36 = iconst.i32 16
            v37 = ushr.i32 v30, v36  ; v36 = 16
            v38 = iconst.i32 255
            v39 = band v37, v38  ; v38 = 255
            v40 = iconst.i32 255
            v41 = icmp eq v39, v40  ; v40 = 255
            v42 = iconst.i64 8
            v43 = iconst.i64 16
            v44 = select v41, v43, v42  ; v43 = 16, v42 = 8
            v45 = iadd.i64 v27, v44
            v46 = load.i64 notrap aligned v45
            v47 = iconst.i64 1
            v48 = sshr v46, v47  ; v47 = 1
            v49 = call_indirect sig7, v48(v0, v27, v28)  ; v28 = 1
            jump block6(v49)

        block5:
            v50 = call fn7(v0, v27, v28)  ; v28 = 1
            jump block6(v50)

        block6(v35: i64):
            v51 = load.i8 notrap aligned v0+24
            brif v51, block7, block8(v35)

        block7 cold:
            v53 = load.i64 notrap aligned v0
            v54 = icmp eq v53, v24
            brif v54, block9, block10

        block9 cold:
            v55 = call fn8(v0)
            jump block8(v55)

        block10 cold:
            v56 = call fn9(v0, v24)
            call fn3(v0, v56)
            v57 = iconst.i64 2
            call fn2(v0, v57)  ; v57 = 2
            call fn3(v0, v24)
            call fn3(v0, v2)
            v58 = iconst.i64 1
            return v58  ; v58 = 1

        block8(v52: i64):
            v59 = call fn10(v0, v24, v52)
            v60 = load.i8 notrap aligned v0+24
            brif v60, block11, block12

        block11 cold:
            v61 = iconst.i64 2
            call fn2(v0, v61)  ; v61 = 2
            call fn3(v0, v2)
            v62 = iconst.i64 1
            return v62  ; v62 = 1

        block12:
            v63 = iconst.i64 1
            v64 = band.i64 v59, v63  ; v63 = 1
            brif v64, block16, block17

        block17:
            call fn12(v0, v59)
            v65 = load.i32 notrap aligned v59
            v66 = iconst.i32 -2
            v67 = iadd v65, v66  ; v66 = -2
            v68 = iconst.i32 0x7fff_fffd
            v69 = icmp ult v67, v68  ; v68 = 0x7fff_fffd
            brif v69, block13, block14

        block13:
            v70 = iconst.i32 -1
            v71 = iadd.i32 v65, v70  ; v70 = -1
            store notrap aligned v71, v59
            jump block16

        block14:
            brif.i32 v65, block15, block16

        block15 cold:
            call fn11(v0, v59)
            jump block16

        block16:
            return v2
        }
        ");
    }

    #[test]
    fn a_yield_point_call() {
        let text = "\
yields fn g(x.0: tobj) -> tobj {
  return x.0
}
yields fn f(x.0: tobj) -> tobj {
  let r.1: tobj = call g(x.0)
  let y.2: int = prim yielding()
  switch y.2 { 0 -> b2, _ -> b1 }
b1:
  decref r.1
  return ()
b2:
  return r.1
}
";

        insta::assert_snapshot!(twin(text, "f"), @"
        function %f:f(i64, i64) -> i64 tail {
            sig0 = (i64, i64) -> i64 tail
            sig1 = (i64, i64, i64) -> i64 c
            sig2 = (i64, i64) c
            sig3 = (i64, i64) c
            sig4 = (i64, i64) -> i64 tail
            sig5 = (i64, i64) c
            sig6 = (i64, i64) c
            fn0 = colocated %f:f sig0
            fn1 = %eml_nrt_safepoint sig1
            fn2 = %eml_rt_audit_count sig2
            fn3 = %eml_rt_decref sig3
            fn4 = colocated %f:g sig4
            fn5 = %eml_rt_decref_slow sig5
            fn6 = %eml_rt_audit_decref sig6

        block0(v0: i64, v1: i64):
            v2 = load.i64 notrap aligned v0+40
            v3 = load.i32 notrap aligned v2
            v4 = iconst.i32 0
            v5 = icmp ne v3, v4  ; v4 = 0
            v6 = get_stack_pointer.i64 
            v7 = load.i64 notrap aligned v0+48
            v8 = icmp ult v6, v7
            v9 = bor v5, v8
            brif v9, block1, block3

        block1 cold:
            v10 = func_addr.i64 fn0
            v11 = iconst.i64 0
            v12 = call fn1(v0, v10, v11)  ; v11 = 0
            v13 = iconst.i64 0
            v14 = icmp eq v12, v13  ; v13 = 0
            brif v14, block3, block2

        block2 cold:
            v15 = iconst.i64 2
            call fn2(v0, v15)  ; v15 = 2
            call fn3(v0, v1)
            v16 = iconst.i64 1
            return v16  ; v16 = 1

        block3:
            v17 = call fn4(v0, v1)
            v18 = load.i8 notrap aligned v0+24
            brif v18, block6, block7

        block6 cold:
            v19 = iconst.i64 2
            call fn2(v0, v19)  ; v19 = 2
            v20 = iconst.i64 1
            return v20  ; v20 = 1

        block7:
            v21 = load.i64 notrap aligned v0
            v22 = iconst.i64 1
            v23 = icmp ne v21, v22  ; v22 = 1
            v24 = uextend.i64 v23
            brif v24, block4, block5

        block4:
            v25 = iconst.i64 1
            v26 = band.i64 v17, v25  ; v25 = 1
            brif v26, block11, block12

        block12:
            call fn6(v0, v17)
            v27 = load.i32 notrap aligned v17
            v28 = iconst.i32 -2
            v29 = iadd v27, v28  ; v28 = -2
            v30 = iconst.i32 0x7fff_fffd
            v31 = icmp ult v29, v30  ; v30 = 0x7fff_fffd
            brif v31, block8, block9

        block8:
            v32 = iconst.i32 -1
            v33 = iadd.i32 v27, v32  ; v32 = -1
            store notrap aligned v33, v17
            jump block11

        block9:
            brif.i32 v27, block10, block11

        block10 cold:
            call fn5(v0, v17)
            jump block11

        block11:
            v34 = iconst.i64 1
            return v34  ; v34 = 1

        block5:
            return v17
        }
        ");
    }

    #[test]
    fn the_raise_helper() {
        let text = "fn main() -> unit {\n  return ()\n}\n";

        insta::assert_snapshot!(twin(text, "eml$raise"), @"
        function %eml$raise(i64, i64, i64, i64) tail {
            ss0 = explicit_slot 32, align = 8
            sig0 = (i64, i64, i64, i64, i64) -> i64 c
            sig1 = (i64, i64, i64) c
            sig2 = (i64, i64, i64, i64) -> i64 tail
            sig3 = (i64, i64, i64, i64, i64) -> i64 tail
            fn0 = %eml_nrt_failed sig0
            fn1 = %eml_rt_audit_code sig1

        block0(v0: i64, v1: i64, v2: i64, v3: i64):
            v4 = stack_addr.i64 ss0
            v5 = call fn0(v0, v1, v2, v3, v4)
            v6 = iconst.i64 0
            v7 = icmp eq v5, v6  ; v6 = 0
            brif v7, block3, block4

        block3:
            v8 = stack_addr.i64 ss0
            v9 = load.i64 notrap v8
            v10 = stack_addr.i64 ss0+8
            v11 = load.i64 notrap v10
            v12 = stack_addr.i64 ss0+16
            v13 = load.i64 notrap v12
            v14 = iconst.i64 2
            call fn1(v0, v9, v14)  ; v14 = 2
            v15 = load.i32 notrap aligned v9+4
            v16 = iconst.i32 16
            v17 = ushr v15, v16  ; v16 = 16
            v18 = iconst.i32 255
            v19 = band v17, v18  ; v18 = 255
            v20 = iconst.i32 255
            v21 = icmp eq v19, v20  ; v20 = 255
            v22 = iconst.i64 8
            v23 = iconst.i64 16
            v24 = select v21, v23, v22  ; v23 = 16, v22 = 8
            v25 = iadd v9, v24
            v26 = load.i64 notrap aligned v25
            v27 = iconst.i64 1
            v28 = sshr v26, v27  ; v27 = 1
            v29 = call_indirect sig2, v28(v0, v9, v11, v13)
            v30 = load.i8 notrap aligned v0+24
            brif v30, block5, block6

        block5 cold:
            jump block1

        block6:
            jump block2

        block4:
            v31 = iconst.i64 1
            v32 = icmp.i64 eq v5, v31  ; v31 = 1
            brif v32, block7, block8

        block7:
            v33 = stack_addr.i64 ss0
            v34 = load.i64 notrap v33
            v35 = stack_addr.i64 ss0+8
            v36 = load.i64 notrap v35
            v37 = stack_addr.i64 ss0+16
            v38 = load.i64 notrap v37
            v39 = stack_addr.i64 ss0+24
            v40 = load.i64 notrap v39
            v41 = iconst.i64 3
            call fn1(v0, v34, v41)  ; v41 = 3
            v42 = load.i32 notrap aligned v34+4
            v43 = iconst.i32 16
            v44 = ushr v42, v43  ; v43 = 16
            v45 = iconst.i32 255
            v46 = band v44, v45  ; v45 = 255
            v47 = iconst.i32 255
            v48 = icmp eq v46, v47  ; v47 = 255
            v49 = iconst.i64 8
            v50 = iconst.i64 16
            v51 = select v48, v50, v49  ; v50 = 16, v49 = 8
            v52 = iadd v34, v51
            v53 = load.i64 notrap aligned v52
            v54 = iconst.i64 1
            v55 = sshr v53, v54  ; v54 = 1
            v56 = call_indirect sig3, v55(v0, v34, v36, v38, v40)
            v57 = load.i8 notrap aligned v0+24
            brif v57, block9, block10

        block9 cold:
            jump block1

        block10:
            jump block2

        block8:
            jump block1

        block2 cold:
            trap user1

        block1:
            return
        }
        ");
    }

    #[test]
    fn a_release() {
        insta::assert_snapshot!(twin(RELEASES, "f"), @"
        function %f:f(i64, i64) -> i64 tail {
            gv0 = symbol colocated %k:10
            sig0 = (i64, i64) -> i64 tail
            sig1 = (i64, i64, i64) -> i64 c
            sig2 = (i64, i64) c
            sig3 = (i64, i64) c
            sig4 = (i64, i64, i64, i64, i64) c
            sig5 = (i64, i64, i64) c
            sig6 = (i64, i64, i64) c
            sig7 = (i64, i64) c
            sig8 = (i64, i64) c
            sig9 = (i64, i64) c
            sig10 = (i64, i64) c
            sig11 = (i64, i64, i64, i64) c
            fn0 = colocated %f:f sig0
            fn1 = %eml_nrt_safepoint sig1
            fn2 = %eml_rt_audit_count sig2
            fn3 = %eml_rt_decref sig3
            fn4 = %eml_rt_audit_data sig4
            fn5 = %eml_rt_audit_field sig5
            fn6 = %eml_rt_audit_raw sig6
            fn7 = %eml_rt_audit_unique sig7
            fn8 = %eml_rt_decref_slow sig8
            fn9 = %eml_rt_audit_decref sig9
            fn10 = %eml_rt_free_block sig10
            fn11 = %eml_rt_release sig11

        block0(v0: i64, v1: i64):
            v2 = load.i64 notrap aligned v0+40
            v3 = load.i32 notrap aligned v2
            v4 = iconst.i32 0
            v5 = icmp ne v3, v4  ; v4 = 0
            v6 = get_stack_pointer.i64 
            v7 = load.i64 notrap aligned v0+48
            v8 = icmp ult v6, v7
            v9 = bor v5, v8
            brif v9, block1, block3

        block1 cold:
            v10 = func_addr.i64 fn0
            v11 = iconst.i64 0
            v12 = call fn1(v0, v10, v11)  ; v11 = 0
            v13 = iconst.i64 0
            v14 = icmp eq v12, v13  ; v13 = 0
            brif v14, block3, block2

        block2 cold:
            v15 = iconst.i64 2
            call fn2(v0, v15)  ; v15 = 2
            call fn3(v0, v1)
            v16 = iconst.i64 1
            return v16  ; v16 = 1

        block3:
            v17 = iconst.i64 0
            v18 = iconst.i64 2
            v19 = iconst.i64 0
            call fn4(v0, v1, v17, v18, v19)  ; v17 = 0, v18 = 2, v19 = 0
            v20 = iconst.i64 0
            call fn5(v0, v1, v20)  ; v20 = 0
            v21 = load.i64 notrap aligned v1+8
            v22 = iconst.i64 1
            call fn5(v0, v1, v22)  ; v22 = 1
            v23 = load.i64 notrap aligned v1+16
            v24 = iconst.i64 2
            call fn6(v0, v1, v24)  ; v24 = 2
            v25 = load.i64 notrap aligned v1+24
            v26 = iconst.i64 0
            v27 = iconst.i64 2
            v28 = iconst.i64 2
            call fn4(v0, v1, v26, v27, v28)  ; v26 = 0, v27 = 2, v28 = 2
            call fn7(v0, v1)
            v29 = load.i32 notrap aligned v1
            v30 = iconst.i32 1
            v31 = icmp eq v29, v30  ; v30 = 1
            brif v31, block5, block6

        block5:
            v32 = iconst.i64 1
            call fn5(v0, v1, v32)  ; v32 = 1
            v33 = load.i64 notrap aligned v1+16
            call fn9(v0, v33)
            v34 = load.i32 notrap aligned v33
            v35 = iconst.i32 -2
            v36 = iadd v34, v35  ; v35 = -2
            v37 = iconst.i32 0x7fff_fffd
            v38 = icmp ult v36, v37  ; v37 = 0x7fff_fffd
            brif v38, block7, block8

        block7:
            v39 = iconst.i32 -1
            v40 = iadd.i32 v34, v39  ; v39 = -1
            store notrap aligned v40, v33
            jump block10

        block8:
            brif.i32 v34, block9, block10

        block9 cold:
            call fn8(v0, v33)
            jump block10

        block10:
            call fn10(v0, v1)
            jump block4

        block6 cold:
            v41 = symbol_value.i64 gv0
            v42 = iconst.i64 2
            call fn11(v0, v1, v41, v42)  ; v42 = 2
            jump block4

        block4:
            return v21
        }
        ");
        insta::assert_snapshot!(twin(RELEASES, "g"), @"
        function %f:g(i64, i64) -> i64 tail {
            gv0 = symbol colocated %k:10
            sig0 = (i64, i64) -> i64 tail
            sig1 = (i64, i64, i64) -> i64 c
            sig2 = (i64, i64) c
            sig3 = (i64, i64) c
            sig4 = (i64, i64, i64, i64, i64) c
            sig5 = (i64, i64, i64) c
            sig6 = (i64, i64, i64) c
            sig7 = (i64, i64) c
            sig8 = (i64, i64) c
            sig9 = (i64, i64) c
            sig10 = (i64, i64) -> i64 c
            sig11 = (i64, i64, i64, i64) c
            sig12 = (i64, i64) c
            fn0 = colocated %f:g sig0
            fn1 = %eml_nrt_safepoint sig1
            fn2 = %eml_rt_audit_count sig2
            fn3 = %eml_rt_decref sig3
            fn4 = %eml_rt_audit_data sig4
            fn5 = %eml_rt_audit_field sig5
            fn6 = %eml_rt_audit_raw sig6
            fn7 = %eml_rt_audit_unique sig7
            fn8 = %eml_rt_decref_slow sig8
            fn9 = %eml_rt_audit_decref sig9
            fn10 = %eml_rt_into_token sig10
            fn11 = %eml_rt_release sig11
            fn12 = %eml_rt_drop_token sig12

        block0(v0: i64, v1: i64):
            v2 = load.i64 notrap aligned v0+40
            v3 = load.i32 notrap aligned v2
            v4 = iconst.i32 0
            v5 = icmp ne v3, v4  ; v4 = 0
            v6 = get_stack_pointer.i64 
            v7 = load.i64 notrap aligned v0+48
            v8 = icmp ult v6, v7
            v9 = bor v5, v8
            brif v9, block1, block3

        block1 cold:
            v10 = func_addr.i64 fn0
            v11 = iconst.i64 0
            v12 = call fn1(v0, v10, v11)  ; v11 = 0
            v13 = iconst.i64 0
            v14 = icmp eq v12, v13  ; v13 = 0
            brif v14, block3, block2

        block2 cold:
            v15 = iconst.i64 2
            call fn2(v0, v15)  ; v15 = 2
            call fn3(v0, v1)
            v16 = iconst.i64 1
            return v16  ; v16 = 1

        block3:
            v17 = iconst.i64 0
            v18 = iconst.i64 2
            v19 = iconst.i64 0
            call fn4(v0, v1, v17, v18, v19)  ; v17 = 0, v18 = 2, v19 = 0
            v20 = iconst.i64 0
            call fn5(v0, v1, v20)  ; v20 = 0
            v21 = load.i64 notrap aligned v1+8
            v22 = iconst.i64 1
            call fn5(v0, v1, v22)  ; v22 = 1
            v23 = load.i64 notrap aligned v1+16
            v24 = iconst.i64 2
            call fn6(v0, v1, v24)  ; v24 = 2
            v25 = load.i64 notrap aligned v1+24
            v27 = iconst.i64 0
            v28 = iconst.i64 2
            v29 = iconst.i64 2
            call fn4(v0, v1, v27, v28, v29)  ; v27 = 0, v28 = 2, v29 = 2
            call fn7(v0, v1)
            v30 = load.i32 notrap aligned v1
            v31 = iconst.i32 1
            v32 = icmp eq v30, v31  ; v31 = 1
            brif v32, block5, block6

        block5:
            v33 = iconst.i64 1
            call fn5(v0, v1, v33)  ; v33 = 1
            v34 = load.i64 notrap aligned v1+16
            call fn9(v0, v34)
            v35 = load.i32 notrap aligned v34
            v36 = iconst.i32 -2
            v37 = iadd v35, v36  ; v36 = -2
            v38 = iconst.i32 0x7fff_fffd
            v39 = icmp ult v37, v38  ; v38 = 0x7fff_fffd
            brif v39, block7, block8

        block7:
            v40 = iconst.i32 -1
            v41 = iadd.i32 v35, v40  ; v40 = -1
            store notrap aligned v41, v34
            jump block10

        block8:
            brif.i32 v35, block9, block10

        block9 cold:
            call fn8(v0, v34)
            jump block10

        block10:
            v42 = call fn10(v0, v1)
            jump block4(v42)

        block6 cold:
            v43 = symbol_value.i64 gv0
            v44 = iconst.i64 2
            call fn11(v0, v1, v43, v44)  ; v44 = 2
            v45 = iconst.i64 0
            jump block4(v45)  ; v45 = 0

        block4(v26: i64):
            call fn12(v0, v26)
            return v21
        }
        ");
    }

    #[test]
    fn an_effect_lookup() {
        let text = "\
effect Counter { get(unit) -> tobj }
fn f() -> tobj {
  let r.0: tobj = clause Counter.get(())
  return r.0
}
";

        insta::assert_snapshot!(twin(text, "f"), @"
        function %f:f(i64) -> i64 tail {
            gv0 = symbol colocated %e:Counter
            sig0 = (i64) -> i64 tail
            sig1 = (i64, i64, i64) -> i64 c
            sig2 = (i64, i64) c
            sig3 = (i64, i64) -> i64 c
            sig4 = (i64, i64, i64) -> i64 c
            sig5 = (i64, i64, i64) c
            sig6 = (i64, i64, i64, i64) -> i64 tail
            fn0 = colocated %f:f sig0
            fn1 = %eml_nrt_safepoint sig1
            fn2 = %eml_rt_audit_count sig2
            fn3 = %eml_rt_ev_lookup sig3
            fn4 = %eml_rt_ev_clause sig4
            fn5 = %eml_rt_audit_code sig5

        block0(v0: i64):
            v1 = load.i64 notrap aligned v0+40
            v2 = load.i32 notrap aligned v1
            v3 = iconst.i32 0
            v4 = icmp ne v2, v3  ; v3 = 0
            v5 = get_stack_pointer.i64 
            v6 = load.i64 notrap aligned v0+48
            v7 = icmp ult v5, v6
            v8 = bor v4, v7
            brif v8, block1, block3

        block1 cold:
            v9 = func_addr.i64 fn0
            v10 = iconst.i64 0
            v11 = call fn1(v0, v9, v10)  ; v10 = 0
            v12 = iconst.i64 0
            v13 = icmp eq v11, v12  ; v12 = 0
            brif v13, block3, block2

        block2 cold:
            v14 = iconst.i64 2
            call fn2(v0, v14)  ; v14 = 2
            v15 = iconst.i64 1
            return v15  ; v15 = 1

        block3:
            v16 = symbol_value.i64 gv0
            v17 = load.i64 notrap aligned v16
            v18 = call fn3(v0, v17)
            v19 = iconst.i64 0
            v20 = call fn4(v0, v18, v19)  ; v19 = 0
            v21 = iconst.i64 1
            v22 = iconst.i64 2
            call fn5(v0, v20, v22)  ; v22 = 2
            v23 = load.i32 notrap aligned v20+4
            v24 = iconst.i32 16
            v25 = ushr v23, v24  ; v24 = 16
            v26 = iconst.i32 255
            v27 = band v25, v26  ; v26 = 255
            v28 = iconst.i32 255
            v29 = icmp eq v27, v28  ; v28 = 255
            v30 = iconst.i64 8
            v31 = iconst.i64 16
            v32 = select v29, v31, v30  ; v31 = 16, v30 = 8
            v33 = iadd v20, v32
            v34 = load.i64 notrap aligned v33
            v35 = iconst.i64 1
            v36 = sshr v34, v35  ; v35 = 1
            v37 = call_indirect sig6, v36(v0, v20, v18, v21)  ; v21 = 1
            v38 = load.i8 notrap aligned v0+24
            brif v38, block4, block5

        block4 cold:
            v39 = iconst.i64 2
            call fn2(v0, v39)  ; v39 = 2
            v40 = iconst.i64 1
            return v40  ; v40 = 1

        block5:
            return v37
        }
        ");
    }
}
