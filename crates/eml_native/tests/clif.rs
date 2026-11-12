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
        sig0 = (i64, i64) -> i64 c
        sig1 = (i64, i64, i64) -> i64 tail
        sig2 = (i64, i64, i64, i64) tail
        sig3 = (i64, i64) c
        fn0 = %eml_x_Println sig0
        fn1 = colocated %f:f sig1
        fn2 = colocated %eml$raise sig2
        fn3 = %eml_rt_decref sig3

    block0(v0: i64, v1: i64, v2: i64):
        v3 = call fn0(v0, v1)
        v4 = load.i64 notrap aligned v0+32
        brif v4, block1, block3

    block1 cold:
        v5 = func_addr.i64 fn1
        v6 = iconst.i64 0
        v7 = iconst.i64 2
        call fn2(v0, v5, v6, v7)  ; v6 = 0, v7 = 2
        jump block2

    block2 cold:
        call fn3(v0, v2)
        v8 = iconst.i64 1
        return v8  ; v8 = 1

    block3:
        v9 = call fn0(v0, v2)
        v10 = load.i64 notrap aligned v0+32
        brif v10, block4, block6

    block4 cold:
        v11 = func_addr.i64 fn1
        v12 = iconst.i64 1
        v13 = iconst.i64 2
        call fn2(v0, v11, v12, v13)  ; v12 = 1, v13 = 2
        jump block5

    block5 cold:
        v14 = iconst.i64 1
        return v14  ; v14 = 1

    block6:
        return v9
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
        sig0 = (i64, i64) -> i64 c
        fn0 = %eml_x_IntShow sig0

    block0(v0: i64, v1: i64):
        v2 = call fn0(v0, v1)
        return v2
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
        sig0 = (i64, i64) c
        sig1 = (i64, i64) c
        sig2 = (i64, i64) -> i64 c
        sig3 = (i64, i64) -> i64 tail
        sig4 = (i64, i64, i64, i64) tail
        fn0 = %eml_x_Exit sig0
        fn1 = %eml_rt_decref sig1
        fn2 = %eml_x_Println sig2
        fn3 = colocated %f:f sig3
        fn4 = colocated %eml$raise sig4

    block0(v0: i64, v1: i64):
        v2 = iconst.i64 3
        call fn0(v0, v2)  ; v2 = 3
        jump block1

    block1 cold:
        call fn1(v0, v1)
        v3 = iconst.i64 1
        return v3  ; v3 = 1

    block2:
        v4 = iconst.i64 1
        v5 = call fn2(v0, v1)
        v6 = load.i64 notrap aligned v0+32
        brif v6, block3, block5

    block3 cold:
        v7 = func_addr.i64 fn3
        v8 = iconst.i64 0
        v9 = iconst.i64 2
        call fn4(v0, v7, v8, v9)  ; v8 = 0, v9 = 2
        jump block4

    block4 cold:
        v10 = iconst.i64 1
        return v10  ; v10 = 1

    block5:
        return v4  ; v4 = 1
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
        sig0 = (i64, i64) -> i64 tail
        sig1 = (i64, i64) c
        sig2 = (i64, i64, i64) -> i64 tail
        fn0 = colocated %f:g sig0
        fn1 = %eml_rt_decref sig1
        fn2 = colocated %f:h sig2

    block0(v0: i64, v1: i64, v2: i64):
        v3 = call fn0(v0, v2)
        v4 = load.i64 notrap aligned v0
        v5 = iconst.i64 1
        v6 = icmp ne v4, v5  ; v5 = 1
        brif v6, block1, block2

    block1 cold:
        call fn1(v0, v1)
        v7 = iconst.i64 1
        return v7  ; v7 = 1

    block2:
        return_call fn2(v0, v1, v3)
    }
    ");
}
