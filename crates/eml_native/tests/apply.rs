//! 関数の値と`apply` (docs/spec/native.md の「関数の値」)。JITしたコードにエントリのstubを通して入るので、Miriでは
//! 流さない。
#![cfg(not(miri))]
use eml_cabi::Linked;
use eml_native::Arg;
use eml_nrt::image::{DATUM_FN, Options};

use crate::common::native::{agree, machine, native, native_options, program};

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
    // 引数が余る末尾の`apply`の輪。`count`は`&pick`を3つの引数に適用し、`eml$apply$3`は`pick`を1つ目で呼んでから、
    // その結果の`&count`と残りの2つで`eml$apply$2`へ末尾で飛ぶ。どの段も末尾呼び出しなので、スタックは伸びない
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
