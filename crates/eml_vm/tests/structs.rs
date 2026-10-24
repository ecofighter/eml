//! 複数の値の呼び出しと戻り、構造体と箱の間の変換 (docs/spec/vm.md の「呼び出しと戻り」、docs/spec/lir.md の「box の挿入」と「ダミーの値」)。構造体はまだソースから作られないので、構造体を持つ手書きの LIR を
//! パスに通して実行する。`apply` の引数と結果の構造体は一様な箱で渡り、型どおりの関数の値は一様な版を通る。ダミーを
//! 受ける命令は、パスの後の形の LIR を手で書いて確かめる。

use eml_vm::CompileError;

use crate::common::{
    compile, disasm_lowered, parse, run_lir_stats, run_lowered_stats, run_unverified,
};

const V: &str = "Vec3{float, float, float}";

/// ヒープは作るときにダミーの物体を1つ確保し、`allocations` はそれも数える。
const DUMMY: u64 = 1;

/// LIR のテキストでは、配置を関数より前に書く。
const LAYOUTS: &str = "\
layout Vec3 { Vec3(float, float, float) }
layout Prelude.Bool { False, True }
";

/// `Vec3` を足す `add` と、結果の葉 x、y、z を `x * 100 + y * 10 + z` にまとめて期待の値と比べ、
/// 合えばその値の文字列を、合わなければ `wrong` を出す `check`。確保をしない出力にするため、文字列は定数である。
fn prelude(expected: u32) -> String {
    format!(
        "\
fn add(a.0: {V}, b.1: {V}) -> {V} {{
  unpack a.0 Vec3 #0(x.2: float, y.3: float, z.4: float)
  unpack b.1 Vec3 #0(u.5: float, v.6: float, w.7: float)
  let p.8: float = extern \"Prelude.Num Float.+\"(x.2, u.5)
  let q.9: float = extern \"Prelude.Num Float.+\"(y.3, v.6)
  let r.10: float = extern \"Prelude.Num Float.+\"(z.4, w.7)
  let s.11: {V} = con Vec3 #0(p.8, q.9, r.10)
  return s.11
}}
fn check(v.0: {V}) -> unit {{
  unpack v.0 Vec3 #0(x.1: float, y.2: float, z.3: float)
  let a.4: float = extern \"Prelude.Num Float.*\"(x.1, 100.0)
  let b.5: float = extern \"Prelude.Num Float.*\"(y.2, 10.0)
  let c.6: float = extern \"Prelude.Num Float.+\"(a.4, b.5)
  let d.7: float = extern \"Prelude.Num Float.+\"(c.6, z.3)
  let e.8: enum = extern \"Prelude.Eq Float.==\"(d.7, {expected}.0)
  switch e.8 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let s.9: obj = const \"wrong\"
  let u.10: unit = extern Prelude.println(s.9)
  return u.10
b2:
  let s.11: obj = const \"{expected}\"
  let u.12: unit = extern Prelude.println(s.11)
  return u.12
}}
"
    )
}

/// 構造体を返す直接の呼び出しは、葉を `RetN` で呼び出し元のスロットに直に返す。構造体の値はどこでも箱にならない
/// ので、ダミーのほかに確保は起きない。
#[test]
fn a_struct_comes_back_in_registers() {
    let text = format!(
        "{LAYOUTS}\
fn main() -> unit {{
  let v.0: {V} = con Vec3 #0(1.0, 2.0, 3.0)
  let w.1: {V} = con Vec3 #0(4.0, 5.0, 6.0)
  let r.2: {V} = call add(v.0, w.1)
  let u.3: unit = call check(r.2)
  return u.3
}}
{}",
        prelude(579)
    );
    let (out, stats) = run_lowered_stats(&text);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "579\n");
    assert_eq!(stats.allocations, DUMMY, "{stats:?}");
    let disasm = disasm_lowered(&text);
    assert!(disasm.contains("call_n ("), "{disasm}");
    assert!(disasm.contains("ret_n ("), "{disasm}");
}

/// 構造体を返す関数が構造体を返す関数を末尾で呼ぶと、呼ばれる側は見出しを引き継ぐ。呼ばれる側の `RetN` は、
/// 見出しの戻り先の直前の `CallN` から結果のスロットを読み、末尾で呼んだ関数の呼び出し元に直に書く。
#[test]
fn a_tail_call_returns_multiple_values_through_the_callers_frame() {
    let text = format!(
        "{LAYOUTS}\
fn main() -> unit {{
  let v.0: {V} = con Vec3 #0(1.0, 2.0, 3.0)
  let r.1: {V} = call twice(v.0)
  let u.2: unit = call check(r.1)
  return u.2
}}
fn twice(v.0: {V}) -> {V} {{
  let r.1: {V} = call add(v.0, v.0)
  return r.1
}}
{}",
        prelude(246)
    );
    let (out, stats) = run_lowered_stats(&text);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "246\n");
    assert_eq!(stats.allocations, DUMMY, "{stats:?}");
    let disasm = disasm_lowered(&text);
    assert!(
        disasm.contains("tail_call add(s0, s1, s2, s0, s1, s2)\n"),
        "{disasm}"
    );
}

/// 余る引数の適用で、2段目の適用が構造体を返す。`apply` の引数と結果は一様なので、構造体は箱で `ApplyRest` の
/// フレームに預けられ、結果も箱でもとの `apply` に戻る。
#[test]
fn apply_rest_returns_multiple_values_to_the_original_apply() {
    let text = format!(
        "{LAYOUTS}\
fn main() -> unit {{
  let v.0: {V} = con Vec3 #0(1.0, 2.0, 3.0)
  let r.1: {V} = apply &scale(2.0, v.0)
  let u.2: unit = call check(r.1)
  return u.2
}}
fn scale(k.0: float) -> tobj {{
  let c.1: tobj = closure times(k.0)
  return c.1
}}
fn times(k.0: float, v.1: {V}) -> {V} {{
  unpack v.1 Vec3 #0(x.2: float, y.3: float, z.4: float)
  let a.5: float = extern \"Prelude.Num Float.*\"(k.0, x.2)
  let b.6: float = extern \"Prelude.Num Float.*\"(k.0, y.3)
  let c.7: float = extern \"Prelude.Num Float.*\"(k.0, z.4)
  let w.8: {V} = con Vec3 #0(a.5, b.6, c.7)
  return w.8
}}
{}",
        prelude(246)
    );
    let (out, stats) = run_lowered_stats(&text);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "246\n");
    // 確保はクロージャと、構造体の引数の箱と結果の箱である
    assert_eq!(stats.allocations, DUMMY + 3, "{stats:?}");
}

/// 構造体を返す関数を `apply` で呼ぶと、一様な版が葉を受けて箱を1つ作る。受けた側は箱から葉を取り出す。
#[test]
fn a_struct_result_crosses_an_apply_in_a_box() {
    let text = format!(
        "{LAYOUTS}\
fn main() -> unit {{
  let r.0: {V} = apply &make(())
  let u.1: unit = call check(r.0)
  return u.1
}}
fn make(u.0: unit) -> {V} {{
  let v.1: {V} = con Vec3 #0(1.0, 2.0, 3.0)
  return v.1
}}
{}",
        prelude(123)
    );
    let (out, stats) = run_lowered_stats(&text);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "123\n");
    assert_eq!(stats.allocations, DUMMY + 1, "{stats:?}");
}

/// `apply` の構造体の引数は箱で渡る。結果が一様な `first` は箱から葉を取り出し、型どおりの `same` の一様な版は
/// 箱から葉を取り出して `same` を呼び、結果をまた箱に入れる。
#[test]
fn struct_arguments_cross_an_apply_in_boxes() {
    let text = format!(
        "{LAYOUTS}\
fn main() -> unit {{
  let v.0: {V} = con Vec3 #0(1.0, 2.0, 3.0)
  let x.1: float = apply &first(v.0)
  let w.2: {V} = con Vec3 #0(x.1, 5.0, 6.0)
  let r.3: {V} = apply &same(w.2)
  let u.4: unit = call check(r.3)
  return u.4
}}
internal uniform from 0 fn first(v.0: {V}) -> float {{
  unpack v.0 Vec3 #0(x.1: float, y.2: float, z.3: float)
  return x.1
}}
fn same(v.0: {V}) -> {V} {{
  return v.0
}}
{}",
        prelude(156)
    );
    let (out, stats) = run_lowered_stats(&text);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "156\n");
    assert_eq!(stats.allocations, DUMMY + 3, "{stats:?}");
}

/// 葉が1つの構造体も、`apply` の引数と結果では `M` の箱で渡る。1回目の `apply` の結果の箱は、取り出さずに2回目の
/// `apply` に渡る (docs/spec/lir.md の「box の挿入」)。
#[test]
fn a_one_leaf_struct_crosses_an_apply_in_a_box() {
    let m = "M{float}";
    let text = format!(
        "\
layout M {{ M(float) }}
fn main() -> unit {{
  let m.0: {m} = con M #0(2.5)
  let r.1: {m} = apply &twice(m.0)
  let b.2: {m} = apply &twice(r.1)
  unpack b.2 M #0(x.3: float)
  let u.4: unit = call show(x.3)
  return u.4
}}
fn twice(m.0: {m}) -> {m} {{
  unpack m.0 M #0(x.1: float)
  let y.2: float = extern \"Prelude.Num Float.+\"(x.1, x.1)
  let n.3: {m} = con M #0(y.2)
  return n.3
}}
fn show(x.0: float) -> unit {{
  let s.1: obj = extern \"Prelude.Show Float.show\"(x.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}}
"
    );
    let (out, stats) = run_lowered_stats(&text);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "10.0\n");
    assert_eq!(stats.allocations, DUMMY + 2, "{stats:?}");
}

/// 構造体の引数の部分適用は、構造体の箱をクロージャに捕まえる。後の適用は構造体の引数を1つと数えて、ちょうどの数の
/// 適用にする。
#[test]
fn partial_application_captures_a_struct_box() {
    let text = format!(
        "{LAYOUTS}\
fn main() -> unit {{
  let v.0: {V} = con Vec3 #0(1.0, 2.0, 3.0)
  let c.1: tobj = apply &add(v.0)
  let w.2: {V} = con Vec3 #0(4.0, 5.0, 6.0)
  let r.3: {V} = apply c.1(w.2)
  let u.4: unit = call check(r.3)
  return u.4
}}
{}",
        prelude(579)
    );
    let (out, stats) = run_lowered_stats(&text);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "579\n");
    assert_eq!(stats.allocations, DUMMY + 3, "{stats:?}");
}

/// `ApplyRest` のフレームが預かる構造体の引数は箱である。1段目が `exit` で中断すると、巻き戻しはその箱を手放す。
/// 手放さなければ検査モードがリークとして報告する。
#[test]
fn an_abort_releases_the_struct_box_held_by_apply_rest() {
    let text = "\
layout Named { Named(obj, float) }
fn main() -> unit {
  let a.0: obj = const \"held\"
  let b.1: obj = const \" here\"
  let s.2: obj = extern \"Prelude.++\"(a.0, b.1)
  let n.3: Named{obj, float} = con Named #0(s.2, 2.5)
  let r.4: Named{obj, float} = apply &stop(3, n.3)
  unpack r.4 Named #0(t.5: obj, x.6: float)
  let u.7: unit = extern Prelude.println(t.5)
  return u.7
}
fn stop(c.0: int) -> tobj {
  let x.1: tobj = extern Prelude.exit(c.0)
  return x.1
}
";
    let (out, stats) = run_lowered_stats(text);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "");
    // `stop`、`ApplyRest`、`main` のフレームで、構造体の箱と文字列を1回ずつ手放す
    assert_eq!(
        (stats.unwound_frames, stats.rc_decrements),
        (3, 2),
        "{stats:?}"
    );
}

/// 特殊化しない handler の下で、構造体の箱を返す一様な関数を `apply` で呼び、その関数が yield する。yield 中は
/// 関数がダミーを返し、構造体の `unbox` はダミーから葉のダミーを書き、`release unboxed` は何もしない。yield しな
/// ければ、同じ道は箱から葉を取り出す。
#[test]
fn a_struct_unbox_on_a_dummy_gives_leaf_dummies() {
    let text = |yielding: bool| {
        let (leave, after) = if yielding {
            (
                "  let u.1: unit = prim yield_to(e.0, (), ())
  let d.2: obj = prim dummy()
  return d.2",
                "  decref n.3
  let k.5: tobj = prim yield_cont_take()
  decref k.5
  let c.6: tobj = prim yield_finish()
  decref c.6
",
            )
        } else {
            (
                "  decref e.0
  let s.1: obj = const \"named\"
  let n.2: obj = con Named #0(s.1, 2.5)
  return n.2",
                "  let v.5: unit = extern Prelude.println(n.3)
",
            )
        };
        format!(
            "\
layout Named {{ Named(obj, float) }}
effect E {{ op(unit) -> unit }}
fn main() -> unit {{
  let w.0: tobj = prim evv_get()
  let e.1: obj = prim ev_new[E]((), w.0)
  let r.2: tobj = apply &leave(e.1)
  let (n.3: obj, x.4: float) = unbox r.2 Named
  release unboxed r.2 Named #0(n.3, _)
{after}  let s.7: obj = extern \"Prelude.Show Float.show\"(x.4)
  let u.8: unit = extern Prelude.println(s.7)
  return u.8
}}
fn leave(e.0: obj) -> obj {{
{leave}
}}
"
        )
    };
    let (out, stats) = run_lir_stats(&text(true));
    stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "0.0\n");

    let (out, stats) = run_lir_stats(&text(false));
    stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "named\n2.5\n");
}

/// 構造体の `unbox` で葉を借りた箱の `release` は、ダミーを受けたら何もしない。token を作る形は空の token を返す。
/// 検査モードは、ダミーでない `BigInt` を構造体の箱として受けたら内部の誤りにする。
#[test]
fn release_unboxed_on_a_dummy_does_nothing() {
    let text = "\
layout Named { Named(obj, float) }
fn main() -> unit {
  let d.0: obj = prim dummy()
  let (n.1: obj, x.2: float) = unbox d.0 Named
  release unboxed d.0 Named #0(n.1, _)
  decref n.1
  let e.3: obj = prim dummy()
  let (_, y.4: float) = unbox e.3 Named
  let t.5: token = release unboxed e.3 Named #0(_, _)
  drop_token t.5
  let s.6: obj = const \"ok\"
  let u.7: unit = extern Prelude.println(s.6)
  return u.7
}
";
    let (out, result) = run_unverified(text);
    result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "ok\n");

    let text = "\
layout Named { Named(obj, float) }
fn main() -> unit {
  let b.0: tobj = box 9223372036854775807
  let t.1: token = release unboxed b.0 Named #0(_, _)
  drop_token t.1
  return ()
}
";
    let (_, result) = run_unverified(text);
    assert_eq!(
        result.unwrap_err().to_string(),
        "internal error: a struct box is a `BigInt` that is not the dummy in `main`"
    );
}

/// 入口の関数は値を1つ返す。`load` は結果が複数の値の入口を断る。
#[test]
fn a_multi_value_entry_is_rejected_at_load() {
    let text = "\
fn main() -> (float, float) {
  return (1.0, 2.0)
}
";
    let error = compile(&parse(text)).err();
    assert_eq!(
        error,
        Some(CompileError::MultiValueEntry {
            function: "main".to_string()
        })
    );
}

/// 箱の欄が `tobj` と宣言された構造体は、スカラーの葉も箱に入れて欄に置く。`BigInt` の大きさの `int`、箱の
/// `float`、ヒープの文字列を持つ箱を、`apply` の一様な結果で受けて共有のまま2回取り出す。共有の箱から取り出すときは、
/// 葉を dup して箱の参照を減らす。最後に取り出すときは、葉を移して箱を空ける。
#[test]
fn a_shared_box_gives_its_scalar_leaves_twice() {
    let t = "(,,){int, float, obj}";
    let text = format!(
        "\
layout (,,) {{ (,,)(tobj, tobj, tobj) }}
fn main() -> unit {{
  let w.0: tobj = apply &make(())
  let x.1: int = apply &first(w.0)
  let y.2: obj = apply &third(w.0)
  let s.3: obj = extern \"Prelude.Show Int.show\"(x.1)
  let u.4: unit = extern Prelude.println(s.3)
  let u.5: unit = extern Prelude.println(y.2)
  return u.5
}}
fn make(u.0: unit) -> {t} {{
  let a.1: obj = const \"he\"
  let b.2: obj = const \"llo\"
  let s.3: obj = extern \"Prelude.++\"(a.1, b.2)
  let p.4: {t} = con (,,) #0(4611686018427387904, 2.5, s.3)
  return p.4
}}
fn first(p.0: {t}) -> int {{
  unpack p.0 (,,) #0(x.1: int, y.2: float, z.3: obj)
  return x.1
}}
fn third(p.0: {t}) -> obj {{
  unpack p.0 (,,) #0(x.1: int, y.2: float, z.3: obj)
  return z.3
}}
"
    );
    let disasm = disasm_lowered(&text);
    assert!(disasm.contains("dup s"), "{disasm}");
    let (out, stats) = run_lowered_stats(&text);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "4611686018427387904\nhello\n");
    // `box` は `make` の結果の `int` と `float` の葉と、`first` の結果の `int` の3つである。`first` と `third` の版が
    // 葉の箱を2つずつ開け、`main` が `first` の結果の箱を開ける
    assert_eq!((stats.boxes, stats.unboxes), (3, 5), "{stats:?}");
}

/// `tobj` と宣言された欄に入った構造体は、箱の中でも箱になる。一様な版が入れ子の箱を作り、共有の箱から2回取り出す
/// たびに、入れ子の箱からも葉を取り出す。
#[test]
fn a_nested_struct_boxes_and_unboxes_its_inner_struct() {
    let seg = "Seg{Vec2{float, float}, int}";
    let text = format!(
        "\
layout Vec2 {{ Vec2(float, float) }}
layout Seg {{ Seg(tobj, int) }}
fn main() -> unit {{
  let w.0: tobj = apply &make(())
  let x.1: float = apply &sum(w.0)
  let y.2: float = apply &sum(w.0)
  let z.3: float = extern \"Prelude.Num Float.+\"(x.1, y.2)
  let s.4: obj = extern \"Prelude.Show Float.show\"(z.3)
  let u.5: unit = extern Prelude.println(s.4)
  return u.5
}}
fn make(u.0: unit) -> {seg} {{
  let v.1: Vec2{{float, float}} = con Vec2 #0(1.5, 2.25)
  let s.2: {seg} = con Seg #0(v.1, 9)
  return s.2
}}
fn sum(s.0: {seg}) -> float {{
  unpack s.0 Seg #0(v.1: Vec2{{float, float}}, n.2: int)
  unpack v.1 Vec2 #0(x.3: float, y.4: float)
  let z.5: float = extern \"Prelude.Num Float.+\"(x.3, y.4)
  return z.5
}}
"
    );
    let disasm = disasm_lowered(&text);
    assert!(disasm.contains("dup s"), "{disasm}");
    let (out, stats) = run_lowered_stats(&text);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "7.5\n");
    // `box` は `sum` の版の `float` の結果の2つで、`main` がどちらも開ける。入れ子の箱は `con` で作る
    assert_eq!((stats.boxes, stats.unboxes), (2, 2), "{stats:?}");
}
