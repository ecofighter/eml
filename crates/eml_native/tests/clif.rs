//! CLIFの固定。生成したコードの速い道と冷たい道の形を、関数1つのコンパイルの前のCLIFで固定する
//! (docs/spec/native.md の「巻き戻しの地点」)。
#![cfg(not(miri))]
use crate::common::native::clif;

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
    // 呼び出しの後で`yielding.target`を確かめ、中断なら持っている`s.0`を手放して仮の値を返す。呼ぶ先はどれも
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
        v18 = load.i64 notrap aligned v0
        v19 = iconst.i64 1
        v20 = icmp ne v18, v19  ; v19 = 1
        brif v20, block4, block5

    block4 cold:
        call fn2(v0, v1)
        v21 = iconst.i64 1
        return v21  ; v21 = 1

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
