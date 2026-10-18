//! reuse の命令の実行を、手書きの LIR で確かめる (docs/spec/vm.md の「reuse」)。token を出すパスがなくても、
//! token を定義する `release`、token を受ける `con`、`drop_token` と巻き戻しの token を VM だけで確かめられる。
//! 文字列はみな定数なので、確保の回数は `con` の分と、ヒープが最初に作る不死のダミーの1回だけになる
//! (docs/spec/vm.md の「`RunStats`」)。

use eml_test_support::execute;
use eml_vm::{RunConfig, RunStats, capture_host};

use crate::common::{parse, run_lir};

/// 検査モードの回数。出力を確かめた後に読む。
fn checked_stats(text: &str) -> RunStats {
    let program = parse(text);
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    let (_, result) = execute(&program, &RunConfig::default().with_debug_heap(true));
    result.unwrap_or_else(|error| panic!("{error}"))
}

const P: &str = "layout P { P(obj, obj) }\n";

/// 一意な `P` の箱を token にし、フィールドを入れ替えた `P` をその箱に作る。
const SWAPPED: &str = "\
fn main() -> unit {
  let a.0: obj = const \"1\"
  let b.1: obj = const \"2\"
  let p.2: obj = con P #0(a.0, b.1)
  unpack p.2 P #0(x.3: obj, y.4: obj)
  let r.5: token = release p.2 P #0(x.3, y.4)
  let q.6: obj = con P #0(y.4, x.3) reuse r.5
  unpack q.6 P #0(u.7: obj, v.8: obj)
  release q.6 P #0(u.7, v.8)
  let o.9: unit = extern Prelude.println(u.7)
  let o.10: unit = extern Prelude.println(v.8)
  return o.10
}
";

#[test]
fn a_unique_box_is_reused() {
    let text = format!("{P}{SWAPPED}");
    assert_eq!(run_lir(&text), ("2\n1\n".to_string(), Ok(())));
    let stats = checked_stats(&text);
    // ダミーと最初の `P` の2回で、2つ目の `P` は確保しない
    assert_eq!((stats.reuses, stats.allocations), (1, 2));
}

/// 共有された箱の token は空なので、`con` は新しく確保し、元の値はそのまま残る。
#[test]
fn a_shared_box_gives_an_empty_token() {
    let text = format!(
        "{P}fn main() -> unit {{
  let a.0: obj = const \"1\"
  let b.1: obj = const \"2\"
  let p.2: obj = con P #0(a.0, b.1)
  dup p.2
  unpack p.2 P #0(x.3: obj, y.4: obj)
  let r.5: token = release p.2 P #0(x.3, y.4)
  let q.6: obj = con P #0(y.4, x.3) reuse r.5
  unpack q.6 P #0(u.7: obj, v.8: obj)
  release q.6 P #0(u.7, v.8)
  let o.9: unit = extern Prelude.println(u.7)
  let o.10: unit = extern Prelude.println(v.8)
  unpack p.2 P #0(s.11: obj, t.12: obj)
  release p.2 P #0(s.11, t.12)
  let o.13: unit = extern Prelude.println(s.11)
  let o.14: unit = extern Prelude.println(t.12)
  return o.14
}}
"
    );
    assert_eq!(run_lir(&text), ("2\n1\n1\n2\n".to_string(), Ok(())));
    let stats = checked_stats(&text);
    // ダミーと2つの `P` の3回
    assert_eq!((stats.reuses, stats.allocations), (0, 3));
}

/// その場の書き換えで書かないフィールドは、箱に残っていた元の値を保つ。
#[test]
fn a_skipped_field_keeps_its_value() {
    let text = format!(
        "{P}fn main() -> unit {{
  let a.0: obj = const \"1\"
  let b.1: obj = const \"2\"
  let p.2: obj = con P #0(a.0, b.1)
  unpack p.2 P #0(x.3: obj, y.4: obj)
  let r.5: token = release p.2 P #0(x.3, y.4)
  decref y.4
  let c.6: obj = const \"3\"
  let q.7: obj = con P #0(=x.3, c.6) reuse r.5 in place
  unpack q.7 P #0(u.8: obj, v.9: obj)
  release q.7 P #0(u.8, v.9)
  let o.10: unit = extern Prelude.println(u.8)
  let o.11: unit = extern Prelude.println(v.9)
  return o.11
}}
"
    );
    assert_eq!(run_lir(&text), ("1\n3\n".to_string(), Ok(())));
    assert_eq!(checked_stats(&text).reuses, 1);
}

/// 別の型の同じ大きさのコンストラクタが箱を使うと、ヘッダのタグが書き直され、switch はそのタグの case に進む。
/// `run_lir` は検査モードと速いモードの両方で流す。
#[test]
fn another_constructor_rewrites_the_header() {
    let text = format!(
        "{P}layout E {{ L(obj), R(obj, obj) }}
fn main() -> unit {{
  let a.0: obj = const \"1\"
  let b.1: obj = const \"2\"
  let p.2: obj = con P #0(a.0, b.1)
  unpack p.2 P #0(x.3: obj, y.4: obj)
  let r.5: token = release p.2 P #0(x.3, y.4)
  let q.6: obj = con E #1(y.4, x.3) reuse r.5
  switch q.6 E {{ #0(z.7: obj) -> b1, #1(u.8: obj, v.9: obj) -> b2 }}
b1:
  release q.6 E #0(z.7)
  decref z.7
  let s.10: obj = const \"left\"
  let o.11: unit = extern Prelude.println(s.10)
  return o.11
b2:
  release q.6 E #1(u.8, v.9)
  let o.12: unit = extern Prelude.println(u.8)
  let o.13: unit = extern Prelude.println(v.9)
  return o.13
}}
"
    );
    assert_eq!(run_lir(&text), ("2\n1\n".to_string(), Ok(())));
    assert_eq!(checked_stats(&text).reuses, 1);
}

/// `exit` の中断が、生きている token を通って巻き戻す。巻き戻しが token を手放さなければ、検査モードの `close` が
/// 箱をリークとして報告する (docs/spec/vm.md の「巻き戻し」)。
#[test]
fn a_live_token_is_dropped_on_exit() {
    let text = format!(
        "{P}fn main() -> unit {{
  let a.0: obj = const \"1\"
  let b.1: obj = const \"2\"
  let p.2: obj = con P #0(a.0, b.1)
  unpack p.2 P #0(x.3: obj, y.4: obj)
  let r.5: token = release p.2 P #0(x.3, y.4)
  let z.6: unit = extern Prelude.exit(3)
  let q.7: obj = con P #0(y.4, x.3) reuse r.5
  decref q.7
  return z.6
}}
"
    );
    let program = parse(&text);
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    for debug_heap in [true, false] {
        let (host, out) = capture_host(Default::default());
        let config = RunConfig::default().with_debug_heap(debug_heap);
        let finished =
            eml_vm::execute(&program, &config, host).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!((finished.exit, out.contents()), (Some(3), String::new()));
    }
}
