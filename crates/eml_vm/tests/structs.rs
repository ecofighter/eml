//! 複数の値の呼び出しと戻り、構造体と箱の間の変換 (docs/spec/vm.md の「呼び出しと戻り」と「変換のフレーム」、docs/spec/lir.md の「ダミーの値」)。構造体はまだソースから作られないので、構造体を持つ手書きの LIR を
//! パスに通して実行する。ダミーを受ける命令は、パスの後の形の LIR を手で書いて確かめる。

use std::sync::Arc;

use eml_rt::{Control, Fast, Limits};
use eml_vm::{CompileError, compile};

use crate::common::{disasm_lowered, parse, run_lir_stats, run_lowered_stats, run_unverified};

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

/// 余る引数の適用で、2段目の適用が構造体を返す。2段目は `ApplyRest` のフレームの見出しを引き継ぐので、`RetN` は
/// もとの `apply` の命令から結果のスロットを読む。`ApplyRest` のフレームは構造体の引数を葉の語のまま預かる。
#[test]
fn apply_rest_returns_multiple_values_to_the_original_apply() {
    let text = format!(
        "{LAYOUTS}\
fn main() -> unit {{
  let v.0: {V} = con Vec3 #0(1.0, 2.0, 3.0)
  let r.1: {V} = apply &scale(2.0: float, v.0)
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
    // 確保はクロージャだけで、構造体は箱にならない
    assert_eq!(
        (stats.allocations, stats.conversions),
        (DUMMY + 1, 0),
        "{stats:?}"
    );
}

/// 一様な場所が構造体を返す関数を適用すると、変換のフレームが葉を受けて箱を1つ作る。受けた側は箱から葉を取り出す。
#[test]
fn a_struct_result_crosses_a_conversion_frame() {
    let text = format!(
        "{LAYOUTS}\
fn main() -> unit {{
  let r.0: {V} = uniform apply &make(())
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
    assert_eq!(
        (stats.conversions, stats.boxes, stats.allocations),
        (1, 1, DUMMY + 1),
        "{stats:?}"
    );
}

/// 結果が一様な関数を、構造体を受ける型どおりの場所が適用すると、変換のフレームが箱を消費して葉を返す。葉は2つ
/// 以上なので、変換のフレームも `RetN` と同じく `apply` の命令から結果のスロットを読む。
#[test]
fn a_box_result_comes_back_as_leaves_through_a_conversion_frame() {
    let text = format!(
        "{LAYOUTS}\
fn main() -> unit {{
  let r.0: {V} = apply &make((): unit)
  let u.1: unit = call check(r.0)
  return u.1
}}
internal uniform from 0 fn make(u.0: unit) -> {V} {{
  let v.1: {V} = con Vec3 #0(1.0, 2.0, 3.0)
  return v.1
}}
{}",
        prelude(123)
    );
    let (out, stats) = run_lowered_stats(&text);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "123\n");
    // `make` は結果が一様なので自分で箱を作り、変換のフレームがそれを取り出して空ける
    assert_eq!(
        (stats.conversions, stats.unboxes, stats.allocations),
        (1, 1, DUMMY + 1),
        "{stats:?}"
    );
}

/// 構造体の引数は、呼ばれる関数の引数の Repr へ `apply` が変える。型どおりの場所から一様な引数の関数へは葉を箱に
/// 入れ、一様な場所から型どおりの関数へは箱から葉を取り出す。
#[test]
fn struct_arguments_convert_to_and_from_boxes() {
    let text = format!(
        "{LAYOUTS}\
fn main() -> unit {{
  let v.0: {V} = con Vec3 #0(1.0, 2.0, 3.0)
  let x.1: float = apply &first(v.0)
  let w.2: {V} = con Vec3 #0(x.1, 5.0, 6.0)
  let r.3: {V} = uniform apply &same(w.2)
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
    // `first` の引数の箱と `float` の結果の箱、`same` の引数の箱と構造体の結果の箱で、4つを作って手放す。`apply` は
    // 2回とも変換する
    assert_eq!(
        (stats.conversions, stats.allocations),
        (2, DUMMY + 4),
        "{stats:?}"
    );
}

/// 種類の等しい構造体の組は語の位置を変えないので、その隣のスカラーの引数だけをその場で箱に入れる。構造体の葉は
/// 箱に入らない (docs/spec/vm.md の「シグネチャ」)。
#[test]
fn a_struct_beside_a_converted_scalar_keeps_its_leaves() {
    let text = format!(
        "{LAYOUTS}\
fn main() -> unit {{
  let v.0: {V} = con Vec3 #0(1.0, 2.0, 3.0)
  let r.1: {V} = apply &shift(v.0, 4.0: float)
  let u.2: unit = call check(r.1)
  return u.2
}}
fn shift(v.0: {V}, k.1: tobj) -> {V} {{
  unpack v.0 Vec3 #0(x.2: float, y.3: float, z.4: float)
  let a.5: float = call plus(x.2, k.1)
  let s.6: {V} = con Vec3 #0(a.5, y.3, z.4)
  return s.6
}}
fn plus(a.0: float, b.1: float) -> float {{
  let c.2: float = extern \"Prelude.Num Float.+\"(a.0, b.1)
  return c.2
}}
{}",
        prelude(523)
    );
    let (out, stats) = run_lowered_stats(&text);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "523\n");
    // 作るのは `4.0` の箱だけである
    assert_eq!(
        (stats.conversions, stats.allocations),
        (1, DUMMY + 1),
        "{stats:?}"
    );
}

/// 葉が1つの構造体は1語で渡り、型どおりの場所と関数の間では変換しない。一様な場所からは1つの葉を箱から取り出し、
/// 結果をまた箱に入れる (docs/spec/vm.md の「シグネチャ」)。
#[test]
fn a_one_leaf_struct_crosses_an_apply_in_one_word() {
    let m = "M{float}";
    let text = format!(
        "\
layout M {{ M(float) }}
fn main() -> unit {{
  let m.0: {m} = con M #0(2.5)
  let r.1: {m} = apply &twice(m.0)
  let b.2: {m} = uniform apply &twice(r.1)
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
    // 作るのは、一様な場所の引数の箱と結果の箱と、出力の文字列だけである
    assert_eq!(
        (stats.conversions, stats.allocations),
        (1, DUMMY + 3),
        "{stats:?}"
    );
}

/// 構造体を返す関数が、結果が一様な関数を末尾で適用する。変換のフレームは関数の位置に積まれ、箱から取り出した葉を
/// 関数の呼び出し元の `CallN` のスロットに書く。一様な場所がその関数を適用すると、積んだ変換と末尾の変換の合成は
/// 恒等なので、変換のフレームは外れ、箱がそのまま一様な場所に戻る。
#[test]
fn tail_conversions_reach_struct_results() {
    let text = |main: &str| {
        format!(
            "{LAYOUTS}\
fn main() -> unit {{
{main}  let u.2: unit = call check(r.1)
  return u.2
}}
fn via(k.0: tobj) -> {V} {{
  let p.1: {V} = uniform apply k.0(())
  return p.1
}}
internal uniform from 0 fn make(u.0: unit) -> {V} {{
  let v.1: {V} = con Vec3 #0(1.0, 2.0, 3.0)
  return v.1
}}
{}",
            prelude(123)
        )
    };
    let direct = text("  let r.1: Vec3{float, float, float} = call via(&make)\n");
    let (out, stats) = run_lowered_stats(&direct);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "123\n");
    assert_eq!(
        (stats.conversions, stats.unboxes, stats.allocations),
        (1, 1, DUMMY + 1),
        "{stats:?}"
    );
    let disasm = disasm_lowered(&direct);
    assert!(disasm.contains("tail_apply s0(s1)"), "{disasm}");

    // `main` の一様な場所は構造体から箱への変換を積み、`via` の末尾の適用の箱から構造体への変換と合成して外す
    let uniform = text("  let r.1: Vec3{float, float, float} = uniform apply &via(&make)\n");
    let (out, stats) = run_lowered_stats(&uniform);
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "123\n");
    assert_eq!(
        (stats.conversions, stats.boxes, stats.unboxes),
        (2, 0, 0),
        "{stats:?}"
    );
}

/// 構造体の引数の部分適用は、葉の語をクロージャに捕まえる。`&f` の即値に詰める捕まえた値の数は源の単位なので、
/// 後の適用は構造体の引数を1つと数えて、ちょうどの数の適用にする。
#[test]
fn partial_application_captures_struct_leaves() {
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
    assert_eq!(
        (stats.conversions, stats.allocations),
        (0, DUMMY + 1),
        "{stats:?}"
    );
}

/// `ApplyRest` のフレームが預かる構造体の引数は葉の語で並ぶ。1段目が `exit` で中断すると、巻き戻しは参照の葉だけを
/// 手放す。手放さなければ検査モードがリークとして報告し、生の葉を手放せばビット列をポインタとして読む。
#[test]
fn an_abort_releases_the_reference_leaves_held_by_apply_rest() {
    let text = "\
layout Named { Named(obj, float) }
fn main() -> unit {
  let a.0: obj = const \"held\"
  let b.1: obj = const \" here\"
  let s.2: obj = extern \"Prelude.++\"(a.0, b.1)
  let n.3: Named{obj, float} = con Named #0(s.2, 2.5)
  let r.4: Named{obj, float} = apply &stop(3: int, n.3)
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
    // `stop`、`ApplyRest`、`main` のフレームで、文字列は1回だけ手放す
    assert_eq!(
        (stats.unwound_frames, stats.rc_decrements),
        (3, 1),
        "{stats:?}"
    );
}

/// 特殊化しない handler の下で、一様な場所が構造体を返す関数を適用し、その関数が yield する。変換のフレームは
/// yield 中なので箱を作らずにダミーを渡し、構造体の `unbox` はダミーから葉のダミーを書き、`release unboxed` は
/// 何もしない。yield しなければ、同じ道は箱を作って取り出す。
#[test]
fn a_struct_unbox_on_a_dummy_gives_leaf_dummies() {
    let text = |yielding: bool| {
        let (leave, after) = if yielding {
            (
                "  let u.1: unit = prim yield_to(e.0, (), ())
  let d.2: obj = prim dummy()
  return (d.2, 0.0)",
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
  return (s.1, 2.5)",
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
  let r.2: tobj = uniform apply &leave(e.1)
  let (n.3: obj, x.4: float) = unbox r.2 Named
  release unboxed r.2 Named #0(n.3, _)
{after}  let s.7: obj = extern \"Prelude.Show Float.show\"(x.4)
  let u.8: unit = extern Prelude.println(s.7)
  return u.8
}}
fn leave(e.0: obj) -> (obj, float) source(obj) -> Named{{obj, float}} {{
{leave}
}}
"
        )
    };
    let (out, stats) = run_lir_stats(&text(true));
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "0.0\n");
    assert_eq!((stats.conversions, stats.boxes), (1, 0), "{stats:?}");

    let (out, stats) = run_lir_stats(&text(false));
    let stats = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "named\n2.5\n");
    assert_eq!((stats.conversions, stats.boxes), (1, 1), "{stats:?}");
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
    let control = Arc::new(Control::new(Limits::default()));
    let error = compile::<Fast>(&parse(text), control).err();
    assert_eq!(
        error,
        Some(CompileError::MultiValueEntry {
            function: "main".to_string()
        })
    );
}

/// 箱の欄が `tobj` と宣言された構造体は、スカラーの葉も箱に入れて欄に置く。`BigInt` の大きさの `int`、箱の
/// `float`、ヒープの文字列を持つ箱を、一様な場所が受けて共有のまま2回取り出す。共有の箱から取り出すときは、葉を
/// dup して箱の参照を減らす。最後に取り出すときは、葉を移して箱を空ける。
#[test]
fn a_shared_box_gives_its_scalar_leaves_twice() {
    let t = "(,,){int, float, obj}";
    let text = format!(
        "\
layout (,,) {{ (,,)(tobj, tobj, tobj) }}
fn main() -> unit {{
  let w.0: tobj = uniform apply &make(())
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
    // 箱にするときは構造体の箱と `int`、`float` の葉の箱で3つ、取り出すたびに同じ3つを開ける
    assert_eq!(
        (stats.conversions, stats.boxes, stats.unboxes),
        (3, 3, 6),
        "{stats:?}"
    );
}

/// `tobj` と宣言された欄に入った構造体は、箱の中でも箱になる。変換のフレームが入れ子の箱を作り、共有の箱から2回取り出す
/// たびに、入れ子の箱からも葉を取り出す。
#[test]
fn a_nested_struct_boxes_and_unboxes_its_inner_struct() {
    let seg = "Seg{Vec2{float, float}, int}";
    let text = format!(
        "\
layout Vec2 {{ Vec2(float, float) }}
layout Seg {{ Seg(tobj, int) }}
fn main() -> unit {{
  let w.0: tobj = uniform apply &make(())
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
    // 箱は `Seg` と `Vec2` の2つで、取り出すたびに2つとも開ける
    assert_eq!(
        (stats.conversions, stats.boxes, stats.unboxes),
        (3, 2, 4),
        "{stats:?}"
    );
}
