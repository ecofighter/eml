//! CLIFの固定。生成したコードの速い道と冷たい道の形を、関数1つのコンパイルの前のCLIFで固定する
//! (docs/spec/native.md の「巻き戻しの地点」)。
#![cfg(not(miri))]
use std::ffi::c_void;

use crate::common::native::{clif, clif_linked, program};

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
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = %eml_rt_dup_slow sig3

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
        call fn2(v0, v1)
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
        call fn3(v0, v2)
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
        fn0 = colocated %f:f sig0
        fn1 = %eml_nrt_safepoint sig1
        fn2 = %eml_rt_decref sig2
        fn3 = colocated %eml$apply$1 sig4

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
        call fn2(v0, v23)
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
