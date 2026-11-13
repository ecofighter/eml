//! 関数の値と`apply` (docs/spec/native.md の「関数の値」)。JITしたコードにエントリのstubを通して入るので、Miriでは
//! 流さない。
#![cfg(not(miri))]
use eml_cabi::Linked;
use eml_native::Arg;
use eml_nrt::image::{DATUM_FN, Options};

use crate::common::native::{
    Lin, agree, call, lin_bit, machine, native, native_options, program, visit,
};

#[test]
fn a_static_function_value_takes_no_rc() {
    // `&inc`は不死の静的なクロージャで、`twice`はそれを`dup`して2回適用し、`main`はそれを落とす。runは漏れなく終わり、
    // VMと同じ結果になる。不死の物体の数の操作は0の数の枝を通るので、このテストは数の動きそのものは見ない。見るのは
    // 下の`Image`の行である
    let text = "\
fn inc(x.0: tobj) -> tobj {
  let n.1: int = unbox x.0
  decref x.0
  let m.2: int = extern \"Prelude.Num Int.+\"(n.1, 1)
  let r.3: tobj = box m.2
  return r.3
}
fn twice(f.0: tobj, x.1: tobj) -> tobj {
  dup f.0
  let y.2: tobj = apply f.0(x.1)
  tail apply f.0(y.2)
}
fn entry$main() -> unit {
  let b.0: tobj = box 5
  let r.1: tobj = call twice(&inc, b.0)
  let s.2: tobj = call twice(&inc, r.1)
  let n.3: int = unbox s.2
  decref s.2
  let t.4: obj = extern \"Prelude.Show Int.show\"(n.3)
  let u.5: unit = extern Prelude.println(t.4)
  let d.6: unit = drop &inc
  return u.5
}
";
    agree(text);
    assert_eq!(native(text), ("9\n".to_string(), Ok(None)));

    // 静的なクロージャは`v:inc`の読み込み時の値1つで、`inc`の行の`value`がそのslotを指す。コードは`c0:inc`である
    let program = program(text);
    let mut m = machine();
    m.load(&program, &Linked::default())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let image = m.image().expect("a program is loaded");
    // SAFETY: 表はマシンのもので、マシンが生きている間変わらない
    let data = unsafe { image.data.as_slice() };
    let [datum] = data else {
        panic!("{} data rows", data.len())
    };
    assert_eq!((datum.kind, datum.bits), (DATUM_FN, 1));
    // SAFETY: 上と同じ
    let rows = unsafe { image.functions.as_slice() };
    for row in rows {
        // SAFETY: 上と同じ
        let name = unsafe { row.name.as_str() };
        let expected = if name == "inc" {
            datum.slot
        } else {
            std::ptr::null_mut()
        };
        assert_eq!(row.value, expected, "{name}");
    }
    let symbol = |name: &str| {
        m.symbols()
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("no symbol `{name}`"))
            .address
    };
    assert_eq!(datum.code, symbol("c0:inc"));
    symbol("eml$apply$1");
    m.close();
}

#[test]
fn a_tail_apply_runs_in_constant_stack() {
    // 10^6回の、自分の関数の値への末尾の`apply`。workerのスタックは8 MiBなので、フレームが積み重なれば数十万回で
    // 深さの上限に当たる
    let text = "\
layout Prelude.Bool { False, True }
fn count(n.0: tobj, acc.1: tobj) -> tobj {
  let k.2: int = unbox n.0
  decref n.0
  let z.3: enum = extern \"Prelude.Eq Int.==\"(k.2, 0)
  switch z.3 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let a.4: int = unbox acc.1
  decref acc.1
  let m.5: int = extern \"Prelude.Num Int.-\"(k.2, 1)
  let c.6: int = extern \"Prelude.Num Int.+\"(a.4, 2)
  let p.7: tobj = box m.5
  let q.8: tobj = box c.6
  tail apply &count(p.7, q.8)
b2:
  return acc.1
}
fn start(n.0: int) -> int {
  let b.1: tobj = box n.0
  let z.2: tobj = box 0
  let r.3: tobj = apply &count(b.1, z.2)
  let k.4: int = unbox r.3
  decref r.3
  return k.4
}
fn entry$main() -> unit {
  return ()
}
";
    assert_eq!(count_in_8_mib(text), 2_000_000);
    // 引数が余る末尾の`apply`の輪。`count`は`&pick`を3つの引数に適用する。`eml$apply$3`は`eml$apply$2`を、それは
    // `eml$apply$1`を末尾でなく呼び、`pick`の結果の`&count`を1つ目の残りに適用して、一時の1引数のPAPを作る。入れ子の
    // 段はそこで戻り、外の段がそのPAPと最後の引数で`eml$apply$1`へ末尾で飛び、`eml$pap$1`が`count`を末尾で呼ぶ。
    // 入れ子のフレームは`count`に入る前に消えているので、スタックは伸びない
    let over = text.replace(
        "tail apply &count(p.7, q.8)",
        "let u.9: unit = drop ()\n  tail apply &pick(u.9, p.7, q.8)",
    ) + "\
fn pick(u.0: unit) -> tobj {
  return &count
}
";
    assert_eq!(count_in_8_mib(&over), 2_000_000);
}

/// `text`のプログラムの`start`を、10^6を引数に、8 MiBのworkerのスタックで呼ぶ。
fn count_in_8_mib(text: &str) -> i64 {
    let mut m = machine();
    m.load(&program(text), &Linked::default())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let options = Options {
        stack: 8 << 20,
        ..native_options()
    };
    let result = m
        .call("start", &[Arg::I64(1_000_000)], options)
        .unwrap_or_else(|error| panic!("{error}"));
    m.close();
    let [Arg::I64(n)] = result[..] else {
        panic!("{result:?}")
    };
    n
}

#[test]
fn an_apply_result_is_checked_for_an_abort() {
    // `leave`の`exit`が始めた中断を`apply`の後の確かめが見て、`apply`をまたいで持つ`s.0`を手放す。漏れればcensusが
    // 誤りにする
    let text = "\
fn leave(x.0: tobj) -> tobj {
  let z.1: unit = extern Prelude.exit(3)
  return x.0
}
fn entry$main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(5)
  let b.1: tobj = box 1
  let r.2: tobj = apply &leave(b.1)
  decref r.2
  let u.3: unit = extern Prelude.println(s.0)
  return u.3
}
";
    agree(text);
    assert_eq!(native(text), (String::new(), Ok(Some(3))));
}

/// 引数を1つ取り、2つの引数を待つ関数の値を返す`make`と、その値の`add`。`add`の最初の引数は`++`で作る文字列で、
/// `make`の結果と残りの引数がそろわなければ、それが漏れる。
const MAKE: &str = "\
fn make(x.0: tobj) -> tobj {
  let n.1: int = unbox x.0
  decref x.0
  let s.2: obj = extern \"Prelude.Show Int.show\"(n.1)
  let u.3: unit = extern Prelude.println(s.2)
  return &add
}
fn add(s.0: obj, b.1: tobj) -> unit {
  let y.2: int = unbox b.1
  decref b.1
  let m.3: obj = extern \"Prelude.Show Int.show\"(y.2)
  let r.4: obj = extern \"Prelude.++\"(s.0, m.3)
  let u.5: unit = extern Prelude.println(r.4)
  return u.5
}
fn leave(x.0: tobj) -> tobj {
  let z.1: unit = extern Prelude.exit(3)
  decref x.0
  return &add
}
";

/// `main`の本体。`++`で文字列`s.2`を作り、`callee`を`(1, s.2, 2)`に適用して、その結果を`ending`で返す。
fn excess(callee: &str, ending: Ending) -> String {
    let apply = format!("apply &{callee}(x.3, s.2, y.4)");
    let tail = match ending {
        Ending::Let => format!("  let u.5: unit = {apply}\n  return u.5\n"),
        Ending::Tail => format!("  tail {apply}\n"),
    };
    format!(
        "\
{MAKE}fn entry$main() -> unit {{
  let a.0: obj = const \"sum\"
  let b.1: obj = const \" \"
  let s.2: obj = extern \"Prelude.++\"(a.0, b.1)
  let x.3: tobj = box 1
  let y.4: tobj = box 2
{tail}}}
"
    )
}

#[derive(Clone, Copy)]
enum Ending {
    Let,
    Tail,
}

#[test]
fn an_excess_application_applies_the_result() {
    // `apply`は`make`に1つ目の引数を渡し、その結果の`&add`に残りの2つを渡す
    for ending in [Ending::Let, Ending::Tail] {
        let text = excess("make", ending);
        agree(&text);
        assert_eq!(native(&text), ("1\nsum 2\n".to_string(), Ok(None)));
    }
}

#[test]
fn an_abort_inside_an_excess_application_drops_the_rest() {
    // `leave`は最初の部分の中で`exit`する。残りの引数のうち`s.2`は`++`で作った文字列で、`apply`がそれを1回だけ
    // 手放さなければ、censusが漏れか二重の解放を見つける
    for ending in [Ending::Let, Ending::Tail] {
        let text = excess("leave", ending);
        agree(&text);
        assert_eq!(native(&text), (String::new(), Ok(Some(3))));
    }
}

/// 捕まえた文字列`s.0`と`int`の`n.1`に、引数の`b.2`を足して出力する`greet`。`s.0`は`++`の受け取る参照で、`greet`が
/// 1回呼ばれるごとに1つを手放す。
const GREET: &str = "\
fn greet(s.0: obj, n.1: int, t.2: tobj, b.3: tobj) -> unit {
  let k.4: int = unbox b.3
  decref b.3
  let j.5: int = unbox t.2
  decref t.2
  let m.6: int = extern \"Prelude.Num Int.+\"(n.1, k.4)
  let p.7: int = extern \"Prelude.Num Int.+\"(m.6, j.5)
  let q.8: obj = extern \"Prelude.Show Int.show\"(p.7)
  let r.9: obj = extern \"Prelude.++\"(s.0, q.8)
  let u.10: unit = extern Prelude.println(r.9)
  return u.10
}
";

/// `main`の始め。`++`で作った文字列`s.2`を捕まえたクロージャ`c.4`を、印`mark`で束縛する。
fn greeting(mark: &str) -> String {
    format!(
        "\
{GREET}fn entry$main() -> unit {{
  let a.0: obj = const \"n=\"
  let b.1: obj = extern \"Prelude.Show Int.show\"(4)
  let s.2: obj = extern \"Prelude.++\"(a.0, b.1)
  let t.3: tobj = box 1000
  let c.4: {mark} = closure greet(s.2, 10, t.3)
"
    )
}

#[test]
fn a_closure_is_freed_on_its_last_use() {
    // ただ1つの参照で適用したクロージャは、エントリが捕まえた値を`greet`へ移して箱を空ける。箱が残ればcensusが漏れを
    // 見つけ、箱を空けるときに捕まえた値も手放せば`s.2`が二重に手放される
    for (mark, ending) in [
        ("tobj", Ending::Let),
        ("tobj", Ending::Tail),
        ("tobj lin", Ending::Let),
    ] {
        let apply = "apply c.4(x.5)";
        let tail = match ending {
            Ending::Let => format!("  let u.6: unit = {apply}\n  return u.6\n"),
            Ending::Tail => format!("  tail {apply}\n"),
        };
        let text = greeting(mark) + "  let x.5: tobj = box 5\n" + &tail + "}\n";
        agree(&text);
        assert_eq!(native(&text), ("n=41015\n".to_string(), Ok(None)));
    }
    // クロージャの`lin`ビットは、束縛する変数の印である
    let text = "\
fn k(n.0: int, b.1: tobj) -> tobj {
  return b.1
}
fn entry$main() -> unit {
  return ()
}
fn unr(n.0: int) -> tobj {
  let c.1: tobj = closure k(n.0)
  return c.1
}
fn lin(n.0: int) -> tobj lin {
  let c.1: tobj lin = closure k(n.0)
  return c.1
}
";
    for (name, lin) in [("unr", false), ("lin", true)] {
        let (finished, record) = visit(text, name, lin_bit, Lin { seen: None });
        finished.unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(record.seen, Some((lin, true)), "{name}");
    }
}

#[test]
fn a_shared_closure_dups_its_captures() {
    // 1回目の適用ではクロージャが共有なので、エントリは捕まえた値を`dup`してから箱の参照を手放す。`dup`しなければ、
    // `greet`が`s.2`を手放した後の2回目の適用が、解放した文字列を読む
    let text = greeting("tobj")
        + "\
  dup c.4
  let x.5: tobj = box 5
  let u.6: unit = apply c.4(x.5)
  let y.7: tobj = box 6
  let v.8: unit = apply c.4(y.7)
  return v.8
}
";
    agree(&text);
    assert_eq!(native(&text), ("n=41015\nn=41016\n".to_string(), Ok(None)));
}

#[test]
fn raw_captures_keep_their_bits() {
    // 生の捕まえた値 (`int`、`float`、`enum`、`unit`) は、走査する語の後に引数の順で並ぶ生の語である。`int`の`8`は
    // 物体を指す語のように見えるので、エントリがそれを数えれば、共有の道の`dup`がそこを読む
    let text = "\
layout R { R(int, float, enum) }
fn pack(n.0: int, s.1: obj, x.2: float, u.3: unit, c.4: enum, b.5: tobj) -> obj {
  decref s.1
  decref b.5
  let r.6: obj = con R #0(n.0, x.2, c.4)
  return r.6
}
fn f(n.0: int, x.1: float, c.2: enum) -> (int, float, enum, int, float, enum) {
  let s.3: obj = extern \"Prelude.Show Int.show\"(n.0)
  let k.4: tobj = closure pack(n.0, s.3, x.1, (), c.2)
  dup k.4
  let b.5: tobj = box 1
  let r.6: obj = apply k.4(b.5)
  let b.7: tobj = box 2
  let q.8: obj = apply k.4(b.7)
  unpack r.6 R #0(m.9: int, y.10: float, d.11: enum)
  unpack q.8 R #0(m.12: int, y.13: float, d.14: enum)
  decref r.6
  decref q.8
  return (m.9, y.10, d.11, m.12, y.13, d.14)
}
fn entry$main() -> unit {
  return ()
}
";
    let nan = f64::from_bits(0xfff8_0000_dead_beef);
    for (n, x, c) in [
        (8, -0.0, 1),
        (i64::MIN, nan, 2),
        (i64::MAX, f64::INFINITY, 0),
    ] {
        let got = call(text, "f", &[Arg::I64(n), Arg::F64(x), Arg::I64(c)]);
        let [m1, Arg::F64(y1), d1, m2, Arg::F64(y2), d2] = got[..] else {
            panic!("{got:?}")
        };
        assert_eq!([m1, d1, m2, d2], [Arg::I64(n), Arg::I64(c)].repeat(2)[..]);
        assert_eq!([y1.to_bits(), y2.to_bits()], [x.to_bits(); 2]);
    }
}

#[test]
fn a_closure_with_255_captures() {
    // 捕まえた文字列が`count`個のクロージャは、コードの語と合わせて`count + 1`個の走査する語を持つ。254個以上なら
    // ペイロードの先頭に数の語があり、コードの語と捕まえた値は1語ずれる (docs/spec/object-model.md の「語の位置」)。
    // 253個 (走査する語が254個で数の語なし) と254個 (255個で数の語あり) で、共有の道と最後の参照の道の両方を通す
    for count in [253, 254] {
        let text = wide(count);
        let line = format!("0{}{}\n", count / 2, count - 1);
        agree(&text);
        assert_eq!(native(&text), (line.repeat(2), Ok(None)));
    }
}

/// `count`個の文字列を捕まえた`wide`のクロージャを2回適用するプログラム。`wide`は最初、真ん中、最後の文字列をつないで
/// 出力し、ほかを手放す。
fn wide(count: usize) -> String {
    let mid = count / 2;
    let last = count - 1;
    let params: Vec<String> = (0..count).map(|i| format!("s{i}.{i}: obj")).collect();
    let mut text = format!(
        "fn wide({}, b.{count}: tobj) -> unit {{\n  decref b.{count}\n",
        params.join(", ")
    );
    for i in (1..count).filter(|&i| i != mid && i != last) {
        text += &format!("  decref s{i}.{i}\n");
    }
    let v = count + 1;
    text += &format!(
        "  let r.{v}: obj = extern \"Prelude.++\"(s0.0, s{mid}.{mid})\n\
         \x20 let q.{}: obj = extern \"Prelude.++\"(r.{v}, s{last}.{last})\n\
         \x20 let u.{}: unit = extern Prelude.println(q.{})\n  return u.{}\n}}\n",
        v + 1,
        v + 2,
        v + 1,
        v + 2
    );
    text += "fn entry$main() -> unit {\n";
    for i in 0..count {
        text += &format!("  let s{i}.{i}: obj = extern \"Prelude.Show Int.show\"({i})\n");
    }
    let captures: Vec<String> = (0..count).map(|i| format!("s{i}.{i}")).collect();
    let c = count;
    text += &format!(
        "  let c.{c}: tobj = closure wide({})\n  dup c.{c}\n\
         \x20 let x.{}: tobj = box 1\n  let u.{}: unit = apply c.{c}(x.{})\n\
         \x20 let y.{}: tobj = box 2\n  let w.{}: unit = apply c.{c}(y.{})\n  return w.{}\n}}\n",
        captures.join(", "),
        c + 1,
        c + 2,
        c + 1,
        c + 3,
        c + 4,
        c + 3,
        c + 4
    );
    text
}

#[test]
fn a_wide_over_application_checks_the_stack() {
    // 引数が余る適用の補助の関数は、入れ子にする前にスタックの上限を確かめる。workerのスタックを`real`の256 KiB上までに
    // すると、`eml$apply$180`からの入れ子のフレーム (およそ390 KB) が上限を越え、補助の関数が深さの上限で止める。補助の関数はプログラムの
    // 関数でないので、誤りは`<native code>`を名指す。確かめなければ、`me`の入口が同じ誤りで`me`を名指すか、引数が
    // 十分多ければ`base`を越えてプロセスが止まる
    let (text, start) = wide_over_application(180);
    let mut m = machine();
    m.load(&program(&text), &Linked::default())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let options = Options {
        stack: (3 << 20) + (256 << 10),
        ..native_options()
    };
    let error = m
        .call(start, &[Arg::I64(0)], options)
        .expect_err("the nested helpers run out of stack");
    assert_eq!(
        error.to_string(),
        "the call depth limit is exceeded in `<native code>`"
    );
    m.close();
}

#[test]
#[ignore = "an apply of 600 arguments makes 600 helpers, which take longer than 1 s to compile"]
fn a_very_wide_over_application_stops_with_the_depth_error() {
    // 600個の引数では、補助の関数の入れ子のフレームはおよそ4.3 MBで、`real`より下の3 MiBを越える。上限を確かめなければ、
    // フレームが`base`を越えてプロセスが止まる
    let (text, start) = wide_over_application(600);
    let mut m = machine();
    m.load(&program(&text), &Linked::default())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let options = Options {
        stack: (3 << 20) + (32 << 10),
        ..native_options()
    };
    let error = m
        .call(start, &[Arg::I64(0)], options)
        .expect_err("the nested helpers run out of stack");
    assert_eq!(
        error.to_string(),
        "the call depth limit is exceeded in `<native code>`"
    );
    m.close();
}

/// 自分自身を返す1引数の`me`を`count`個の引数に適用する`start`のプログラムと、`start`の名前。
fn wide_over_application(count: usize) -> (String, &'static str) {
    let mut text = String::from(
        "fn me(x.0: tobj) -> tobj {\n  decref x.0\n  return &me\n}\nfn start(n.0: int) -> int {\n",
    );
    for i in 0..count {
        text += &format!("  let a{i}.{}: tobj = box {i}\n", i + 1);
    }
    let args: Vec<String> = (0..count).map(|i| format!("a{i}.{}", i + 1)).collect();
    let r = count + 1;
    text += &format!(
        "  let r.{r}: tobj = apply &me({})\n  decref r.{r}\n  return 0\n}}\n\
         fn entry$main() -> unit {{\n  return ()\n}}\n",
        args.join(", ")
    );
    (text, "start")
}
