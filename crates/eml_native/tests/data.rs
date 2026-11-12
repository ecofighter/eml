//! 構築子、欄、タグと文字列の`switch`、箱、読み込み時の値 (docs/spec/native.md の「値と定数」、docs/spec/native.md の
//! 「データ」)。JITしたコードにエントリのstubを通して入るので、Miriでは流さない。
#![cfg(not(miri))]
use eml_cabi::Linked;
use eml_native::{Arg, NativeMachine};
use eml_nrt::RunConfig;
use eml_nrt::image::{DATUM_BIG_INT, DATUM_FLOAT, DATUM_STRING};
use eml_rt::{Ctx, Fast, Kind, Word, layout};

use crate::common::native::{Visit, agree, call, call_visited, clif, host, native, program, visit};

const MAIN: &str = "fn entry$main() -> unit {\n  return ()\n}\n";

/// 63ビットに収まる`int`の端。
const MAX_IMM: i64 = Word::MAX_INT;
const MIN_IMM: i64 = Word::MIN_INT;

#[test]
fn con_and_unpack_round_trip_every_field_kind() {
    // 走査する語 (`obj`、`tobj`) と生の語 (`int`、`float`、`enum`) を交互に宣言するので、欄の位置は宣言の順と違う
    let text = format!(
        "\
layout R {{ R(int, obj, float, tobj, enum) }}
{MAIN}fn f(n.0: int, x.1: float, c.2: enum) -> (int, float, enum, int, enum) {{
  let s.3: obj = extern \"Prelude.Show Int.show\"(n.0)
  let b.4: tobj = box n.0
  let r.5: obj = con R #0(n.0, s.3, x.1, b.4, c.2)
  unpack r.5 R #0(m.6: int, t.7: obj, y.8: float, k.9: tobj, d.10: enum)
  let u.11: obj = extern \"Prelude.Show Int.show\"(n.0)
  let e.12: enum = extern \"Prelude.Eq String.==\"(t.7, u.11)
  let j.13: int = unbox k.9
  decref u.11
  decref r.5
  return (m.6, y.8, d.10, j.13, e.12)
}}
"
    );
    let nan = f64::from_bits(0xfff8_0000_dead_beef);
    for (n, x, c) in [
        (0, 0.0, 0),
        (-7, -0.0, 1),
        (i64::MIN, f64::INFINITY, 2),
        (i64::MAX, nan, 0),
        (MAX_IMM, 1.5, 1),
    ] {
        let got = call(&text, "f", &[Arg::I64(n), Arg::F64(x), Arg::I64(c)]);
        let [m, Arg::F64(y), d, j, e] = got[..] else {
            panic!("{got:?}")
        };
        assert_eq!(
            [m, d, j, e],
            [Arg::I64(n), Arg::I64(c), Arg::I64(n), Arg::I64(1)]
        );
        assert_eq!(y.to_bits(), x.to_bits());
    }
}

#[test]
fn a_constructor_with_255_scanned_fields() {
    // 走査する語が255個の形は、ペイロードの先頭に数の語を持つので、どの欄も1語ずれる (docs/spec/object-model.md の
    // 「語の位置」)
    let mut text = "layout W { W(".to_string();
    text += &vec!["tobj"; 255].join(", ");
    text += &format!(", int) }}\n{MAIN}");
    text += "fn f(n.0: int, m.1: int) -> (int, int, int, int) {\n";
    text += "  let a.2: tobj = box n.0\n  let z.3: tobj = box n.0\n";
    for i in 1..254 {
        text += &format!("  let b{i}.{}: tobj = box {}\n", 3 + i, i * 3);
    }
    let fields: Vec<String> = std::iter::once("a.2".to_string())
        .chain((1..254).map(|i| format!("b{i}.{}", 3 + i)))
        .chain(["z.3".to_string(), "m.1".to_string()])
        .collect();
    text += &format!("  let w.300: obj = con W #0({})\n", fields.join(", "));
    let bound: Vec<String> = (0..255)
        .map(|i| format!("f{i}.{}: tobj", 400 + i))
        .chain(["r.700: int".to_string()])
        .collect();
    text += &format!("  unpack w.300 W #0({})\n", bound.join(", "));
    text += "  let p.701: int = unbox f0.400\n  let q.702: int = unbox f128.528\n";
    text += "  let s.703: int = unbox f254.654\n  decref w.300\n";
    text += "  return (p.701, q.702, s.703, r.700)\n}\n";
    for n in [7, i64::MIN] {
        assert_eq!(
            call(&text, "f", &[Arg::I64(n), Arg::I64(-3)]),
            [Arg::I64(n), Arg::I64(128 * 3), Arg::I64(n), Arg::I64(-3)]
        );
    }
}

#[test]
fn switch_on_enum_obj_and_tobj() {
    let text = format!(
        "\
layout Color {{ Red, Green, Blue }}
layout Shape {{ Dot(int), Pair(int, float) }}
layout Opt {{ None, Some(int), Both(int, obj) }}
{MAIN}fn color(c.0: enum) -> int {{
  switch c.0 Color {{ #0 -> b1, #2 -> b2, _ -> b3 }}
b1:
  return 10
b2:
  return 12
b3:
  return 11
}}
fn shape(k.0: int, n.1: int) -> int {{
  switch k.0 {{ 0 -> b1, _ -> b2 }}
b1:
  let d.2: obj = con Shape #0(n.1)
  jump b3(d.2)
b2:
  let p.3: obj = con Shape #1(k.0, 2.5)
  jump b3(p.3)
b3(v.4: obj):
  switch v.4 Shape {{ #0(a.5: int) -> b4, #1(b.6: int, x.7: float) -> b5 }}
b4:
  decref v.4
  return a.5
b5:
  decref v.4
  return b.6
}}
fn opt(k.0: int, n.1: int) -> int {{
  switch k.0 {{ 0 -> b1, 1 -> b2, _ -> b3 }}
b1:
  jump b4(#0)
b2:
  let s.2: tobj = con Opt #1(n.1)
  jump b4(s.2)
b3:
  let t.3: obj = extern \"Prelude.Show Int.show\"(n.1)
  let w.4: tobj = con Opt #2(k.0, t.3)
  jump b4(w.4)
b4(o.5: tobj):
  switch o.5 Opt {{ #0 -> b5, #1(m.6: int) -> b6, #2(j.7: int, u.8: obj) -> b7 }}
b5:
  decref o.5
  return -1
b6:
  decref o.5
  return m.6
b7:
  decref o.5
  return j.7
}}
"
    );
    for (c, want) in [(0, 10), (1, 11), (2, 12)] {
        assert_eq!(call(&text, "color", &[Arg::I64(c)]), [Arg::I64(want)]);
    }
    for (k, want) in [(0, 9), (5, 5)] {
        assert_eq!(
            call(&text, "shape", &[Arg::I64(k), Arg::I64(9)]),
            [Arg::I64(want)]
        );
    }
    for (k, want) in [(0, -1), (1, 9), (2, 2)] {
        assert_eq!(
            call(&text, "opt", &[Arg::I64(k), Arg::I64(9)]),
            [Arg::I64(want)]
        );
    }
}

#[test]
fn box_and_unbox_int_float_enum() {
    let text = format!(
        "{MAIN}\
fn f(n.0: int, x.1: float, c.2: enum) -> (int, float, enum) {{
  let a.3: tobj = box n.0
  let b.4: tobj = box x.1
  let d.5: tobj = box c.2
  let m.6: int = unbox a.3
  let y.7: float = unbox b.4
  let e.8: enum = unbox d.5
  decref a.3
  decref b.4
  decref d.5
  return (m.6, y.7, e.8)
}}
"
    );
    let nan = f64::from_bits(0x7ff4_0000_0000_0001);
    for (n, x, c) in [
        (0, 0.0, 0),
        (-1, -0.0, 1),
        (MAX_IMM, f64::NEG_INFINITY, 2),
        (MAX_IMM + 1, nan, 3),
        (i64::MIN, f64::MIN_POSITIVE, 0),
        (i64::MAX, -2.25, 1),
    ] {
        let got = call(&text, "f", &[Arg::I64(n), Arg::F64(x), Arg::I64(c)]);
        let [m, Arg::F64(y), e] = got[..] else {
            panic!("{got:?}")
        };
        assert_eq!([m, e], [Arg::I64(n), Arg::I64(c)]);
        assert_eq!(y.to_bits(), x.to_bits());
    }
}

/// 箱にした語の見え方。即値ならその語、物体なら種類と値と数である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Boxed {
    Immediate(usize),
    Object { kind: Kind, value: i64, rc: i32 },
}

/// 代役の記録。`ints`のそれぞれについて、生成したコードの箱と`Heap::alloc_int`の箱を`seen`に足す。
struct Boxes {
    ints: Vec<i64>,
    seen: Vec<(i64, Boxed, Boxed, bool)>,
}

/// 箱`w`の見え方。
///
/// # Safety
/// 代役の中で、受け取った`ctx`で呼ぶ。`w`は即値か、このヒープの生きている物体である。
unsafe fn boxed(ctx: *mut Ctx<Fast>, w: Word, ok: &mut bool) -> Boxed {
    if w.is_imm() {
        return Boxed::Immediate(w.bits());
    }
    // SAFETY: 呼ぶ側の約束
    unsafe {
        let heap = (*ctx).heap_mut();
        let kind = heap.meta(w).map(|meta| meta.kind());
        let value = heap.unbox_int(w);
        *ok &= kind.is_ok() && value.is_ok();
        // `Word`は`*mut ()`の透明な包みなので、この変換はポインタの出所を保つ
        let header = std::mem::transmute::<Word, *mut ()>(w);
        Boxed::Object {
            kind: kind.unwrap_or(Kind::Data),
            value: value.unwrap_or(0),
            rc: header
                .wrapping_byte_add(layout::RC_BYTE)
                .cast::<i32>()
                .read(),
        }
    }
}

/// 代役。`int`ごとに試す関数で箱を作り、対の箱を`Heap::alloc_int`で作って、両方の見え方を記録し、両方を手放す。
unsafe extern "C" fn box_each(ctx: *mut Ctx<Fast>, entry: *const u8, _argv: *mut u64) {
    // SAFETY: `visit`が`entry`に記録を置き、この代役だけがそれを使う
    let visit = unsafe { &mut *entry.cast_mut().cast::<Visit<Boxes>>() };
    for n in visit.record.ints.clone() {
        let mut ok = true;
        // SAFETY: 代役の中で、受け取った`ctx`で呼び、関数の呼び出しをまたいで`ctx`への参照を持たない
        let (native, heap) = unsafe {
            let mut argv = [n as u64];
            call_visited(ctx, visit, &mut argv);
            let a = argv.as_ptr().cast::<Word>().read();
            let b = (*ctx).heap_mut().alloc_int(n);
            let seen = (boxed(ctx, a, &mut ok), boxed(ctx, b, &mut ok));
            ok &= (*ctx).heap_mut().decref(a).is_ok();
            ok &= (*ctx).heap_mut().decref(b).is_ok();
            seen
        };
        visit.record.seen.push((n, native, heap, ok));
    }
}

#[test]
fn boxing_an_int_agrees_with_the_heap() {
    let text = format!(
        "{MAIN}fn box_int(n.0: int) -> tobj {{\n  let b.1: tobj = box n.0\n  return b.1\n}}\n"
    );
    let ints = vec![
        MAX_IMM,
        MAX_IMM + 1,
        MIN_IMM,
        MIN_IMM - 1,
        i64::MIN,
        i64::MAX,
    ];
    let record = Boxes {
        ints: ints.clone(),
        seen: Vec::new(),
    };
    let (finished, record) = visit(&text, "box_int", box_each, record);
    finished.unwrap_or_else(|error| panic!("{error}"));
    let want: Vec<(i64, Boxed, Boxed, bool)> = ints
        .iter()
        .map(|&n| {
            let boxed = if (MIN_IMM..=MAX_IMM).contains(&n) {
                Boxed::Immediate(Word::imm(n).bits())
            } else {
                Boxed::Object {
                    kind: Kind::BigInt,
                    value: n,
                    rc: 1,
                }
            };
            (n, boxed, boxed, true)
        })
        .collect();
    assert_eq!(record.seen, want);
}

/// `text`を読み込んだマシンの`Image.data`の行。種類、中身、文字列である。
fn data_rows(text: &str) -> Vec<(u32, u64, String)> {
    let mut machine = NativeMachine::new(&RunConfig::default(), host());
    machine
        .load(&program(text), &Linked::default())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let image = machine.image().expect("a program is loaded");
    // SAFETY: 表はマシンのもので、マシンが生きている間変わらない
    let rows = unsafe { image.data.as_slice() }
        .iter()
        // SAFETY: 上と同じ
        .map(|row| (row.kind, row.bits, unsafe { row.text.as_str() }.to_string()))
        .collect();
    machine.close();
    rows
}

#[test]
fn a_big_int_constant_and_a_float_box_constant() {
    // 63ビットに収まらない`int`と`float`の箱の定数は、凍結した領域の不死の物体である。`decref`はそれを解放しない
    let text = format!(
        "{MAIN}\
fn f() -> (int, int, float, int) {{
  let a.0: tobj = box 4611686018427387904
  let b.1: tobj = box -9223372036854775808
  let c.2: tobj = box -2.5
  let d.3: tobj = box 5
  let m.4: int = unbox a.0
  let n.5: int = unbox b.1
  let x.6: float = unbox c.2
  let k.7: int = unbox d.3
  decref a.0
  decref b.1
  decref c.2
  decref d.3
  return (m.4, n.5, x.6, k.7)
}}
"
    );
    assert_eq!(
        call(&text, "f", &[]),
        [
            Arg::I64(1 << 62),
            Arg::I64(i64::MIN),
            Arg::F64(-2.5),
            Arg::I64(5)
        ]
    );
    // 即値に収まる`5`は読み込み時の値を持たない
    let mut rows = data_rows(&text);
    rows.sort();
    assert_eq!(
        rows,
        [
            (DATUM_BIG_INT, 1 << 62, String::new()),
            (DATUM_BIG_INT, i64::MIN as u64, String::new()),
            (DATUM_FLOAT, (-2.5f64).to_bits(), String::new()),
        ]
    );
}

#[test]
fn equal_constants_share_a_symbol() {
    let text = format!(
        "{MAIN}\
fn f() -> enum {{
  let s.0: obj = const \"hi\"
  let t.1: obj = const \"there\"
  let a.2: tobj = box 4611686018427387904
  let e.3: enum = extern \"Prelude.Eq String.==\"(s.0, t.1)
  decref s.0
  decref t.1
  decref a.2
  return e.3
}}
fn g() -> int {{
  let s.0: obj = const \"hi\"
  let a.1: tobj = box 4611686018427387904
  let b.2: tobj = box 4611686018427387904
  let c.3: tobj = box 0.5
  let m.4: int = unbox b.2
  decref s.0
  decref a.1
  decref b.2
  decref c.3
  return m.4
}}
"
    );
    assert_eq!(call(&text, "f", &[]), [Arg::I64(0)]);
    assert_eq!(call(&text, "g", &[]), [Arg::I64(1 << 62)]);
    let mut rows = data_rows(&text);
    rows.sort();
    assert_eq!(
        rows,
        [
            (DATUM_STRING, 0, "hi".to_string()),
            (DATUM_STRING, 0, "there".to_string()),
            (DATUM_BIG_INT, 1 << 62, String::new()),
            (DATUM_FLOAT, 0.5f64.to_bits(), String::new()),
        ]
    );
    // どちらの関数も、同じ中身の値を同じ名前の記号から読む
    for function in ["f", "g"] {
        let clif = clif(&text, function);
        for symbol in ["%s:hi", "%i:4611686018427387904"] {
            assert!(clif.contains(symbol), "{function} reads {symbol}:\n{clif}");
        }
    }
}

const SAME: &str = "\
fn f() -> unit {
  let s.0: obj = const \"same\"
  let a.1: tobj = box 4611686018427387904
  let c.2: tobj = box 0.25
  let u.3: unit = extern Prelude.println(s.0)
  decref a.1
  decref c.2
  return u.3
}
";

#[test]
fn a_function_with_constants_has_the_same_code_in_two_programs() {
    let alone = format!("{MAIN}{SAME}");
    // ほかの関数は、それぞれ自分の定数を持つので、`f`の記号はmoduleの中で後に宣言される
    let others: String = (0..10)
        .map(|i| {
            format!(
                "fn g{i}() -> unit {{\n  \
                 let s.0: obj = const \"g{i}\"\n  \
                 let a.1: tobj = box {}\n  \
                 let c.2: tobj = box {i}.5\n  \
                 decref a.1\n  decref c.2\n  \
                 let u.3: unit = extern Prelude.println(s.0)\n  \
                 return u.3\n}}\n",
                (1i64 << 62) + i
            )
        })
        .collect();
    let after = format!("{MAIN}{others}{SAME}");
    assert_eq!(clif(&alone, "f"), clif(&after, "f"));
}

#[test]
fn a_string_switch_compares_in_order() {
    // 比べるのは中身なので、実行時に作った`"ab"`も定数の`"ab"`のcaseに合う
    let text = "\
fn pick(borrowed s.0: obj) -> int {
  switch s.0 { \"a\" -> b1, \"ab\" -> b2, \"\" -> b3, _ -> b4 }
b1:
  return 1
b2:
  return 2
b3:
  return 3
b4:
  return 4
}
fn show(n.0: int) -> unit {
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
fn try(borrowed s.0: obj) -> unit {
  let n.1: int = call pick(s.0)
  let u.2: unit = call show(n.1)
  return u.2
}
fn entry$main() -> unit {
  let a.0: obj = const \"a\"
  let u.1: unit = call try(a.0)
  let b.2: obj = const \"ab\"
  let v.3: unit = call try(b.2)
  let c.4: obj = const \"\"
  let w.5: unit = call try(c.4)
  let d.6: obj = const \"b\"
  let x.7: unit = call try(d.6)
  let e.8: obj = extern \"Prelude.++\"(a.0, d.6)
  let y.9: unit = call try(e.8)
  decref b.2
  decref c.4
  decref e.8
  return y.9
}
";
    agree(text);
    assert_eq!(native(text), ("1\n2\n3\n4\n2\n".to_string(), Ok(None)));
    // verifierは同じcaseを2回書かせないので、順は結果に現れない。下ろしは各caseの定数を、比べる所で初めて名指すので、
    // CLIFの記号の宣言の順が比べる順である
    let clif = clif(text, "pick");
    let order: Vec<&str> = clif
        .lines()
        .filter_map(|line| line.split("symbol colocated %s:").nth(1))
        .collect();
    assert_eq!(order, ["a", "ab", ""], "{clif}");
}

const LINS: &str = "\
layout P { P(int) }
fn entry$main() -> unit {
  return ()
}
fn unr(n.0: int) -> obj {
  let p.1: obj = con P #0(n.0)
  return p.1
}
fn lin(n.0: int) -> obj lin {
  let p.1: obj lin = con P #0(n.0)
  return p.1
}
";

/// 代役の記録。試す関数が作った物体の`lin`ビットと、ヒープの呼び出しがどれも成功したかである。
struct Lin {
    seen: Option<(bool, bool)>,
}

/// 代役。試す関数で物体を1つ作り、その`lin`ビットを記録して手放す。
unsafe extern "C" fn lin_bit(ctx: *mut Ctx<Fast>, entry: *const u8, _argv: *mut u64) {
    // SAFETY: `visit`が`entry`に記録を置き、この代役だけがそれを使う
    let visit = unsafe { &mut *entry.cast_mut().cast::<Visit<Lin>>() };
    // SAFETY: 代役の中で、受け取った`ctx`で呼び、関数の呼び出しをまたいで`ctx`への参照を持たない
    let seen = unsafe {
        let mut argv = [5u64];
        call_visited(ctx, visit, &mut argv);
        let w = argv.as_ptr().cast::<Word>().read();
        let lin = (*ctx).heap_mut().meta(w).map(|meta| meta.lin());
        let freed = (*ctx).heap_mut().decref(w).is_ok();
        (lin.unwrap_or(false), lin.is_ok() && freed)
    };
    visit.record.seen = Some(seen);
}

#[test]
fn a_constructor_carries_the_mark_of_its_variable() {
    // `con`の`lin`の引数は束縛する変数の印で、物体の`lin`ビットになる
    for (name, lin) in [("unr", false), ("lin", true)] {
        let (finished, record) = visit(LINS, name, lin_bit, Lin { seen: None });
        finished.unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(record.seen, Some((lin, true)), "{name}");
    }
}
