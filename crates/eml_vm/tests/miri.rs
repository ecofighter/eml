//! Miri で流す小さい実行のテスト (docs/implementation/testing.md の「Miri」)。VM は検証していないコードから `eml_rt`
//! の `unsafe fn` を呼ぶので、核の操作の前提を守っていることを確かめる。メモリの Host だけを使い、ファイルを開かない。
//! 確かめる道: 直接の呼び出しと戻り、`con` と `switch` と `release`、部分適用と余った引数、基本操作 (evidence を作って
//! 引く)、ヒープの文字列の extern、両方のモード。

use eml_vm::{RunConfig, capture_host, execute};

fn run_both(text: &str) -> String {
    let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    let mut outputs = Vec::new();
    for debug_heap in [true, false] {
        let (host, out) = capture_host(Default::default());
        let config = RunConfig::default().with_debug_heap(debug_heap);
        execute(&program, &config, host).unwrap_or_else(|error| panic!("{error}"));
        outputs.push(out.contents());
    }
    assert_eq!(outputs[0], outputs[1]);
    outputs.remove(0)
}

#[test]
fn calls_data_and_strings() {
    let text = "\
layout Option { None, Some(tobj) }
fn main() -> unit {
  let n.0: int = call add(40, 2)
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let t.2: obj = const \"!\"
  let u.3: obj = extern \"Prelude.++\"(s.1, t.2)
  let o.4: tobj = con Option #1(u.3)
  switch o.4 Option { #0 -> b1, #1(x.5: obj) -> b2 }
b1:
  decref o.4
  return ()
b2:
  release o.4 Option #1(x.5)
  let v.6: unit = extern Prelude.println(x.5)
  return v.6
}
fn add(a.0: int, b.1: int) -> int {
  let c.2: int = extern \"Prelude.+\"(a.0, b.1)
  return c.2
}
";
    assert_eq!(run_both(text), "42!\n");
}

/// 引数が足りない適用は、即値の関数からもクロージャからも新しいクロージャを作る (`make_closure`)。
#[test]
fn partial_application_makes_closures() {
    let text = "\
fn main() -> unit {
  let a.0: tobj = box 10
  let d.1: tobj = apply &first3(a.0)
  let b.2: tobj = box 20
  let e.3: tobj = apply d.1(b.2)
  let c.4: tobj = box 30
  let q.5: tobj = apply e.3(c.4)
  let r.6: int = unbox q.5
  decref q.5
  let s.7: obj = extern \"Prelude.Show Int.show\"(r.6)
  let t.8: unit = extern Prelude.println(s.7)
  return t.8
}
fn first3(a.0: tobj, b.1: tobj, c.2: tobj) -> tobj {
  decref b.1
  decref c.2
  return a.0
}
";
    assert_eq!(run_both(text), "10\n");
}

/// 引数が2段余る。`main` の `apply` は末尾ではなく、入れ子は `ApplyRest` の末尾の適用の中で起きる。
#[test]
fn over_application_nests_through_apply_rest() {
    let text = "\
fn main() -> unit {
  let a.0: tobj = box 1
  let b.1: tobj = box 2
  let c.2: tobj = box 3
  let r.3: tobj = apply &make(a.0, b.1, c.2)
  let n.4: int = unbox r.3
  decref r.3
  let s.5: obj = extern \"Prelude.Show Int.show\"(n.4)
  let u.6: unit = extern Prelude.println(s.5)
  return u.6
}
fn make(x.0: tobj) -> tobj {
  let c.1: tobj = closure pick(x.0)
  return c.1
}
fn pick(x.0: tobj, y.1: tobj) -> tobj {
  decref y.1
  let c.2: tobj = closure keep(x.0)
  return c.2
}
fn keep(x.0: tobj, z.1: tobj) -> tobj {
  decref z.1
  return x.0
}
";
    assert_eq!(run_both(text), "1\n");
}

/// 節が evidence の欄の状態を読み書きする。
#[test]
fn an_immediate_clause_reads_and_writes_the_state_of_its_evidence() {
    let text = "effect Counter { get(unit) -> tobj, put(tobj) -> unit }
\
fn main() -> unit {
  let w.0: tobj = prim evv_get()
  dup w.0
  dup w.0
  let s.1: tobj = box 3
  let e.2: obj = prim ev_new[Counter](s.1, w.0)
  let u.3: unit = prim ev_set_clause[Counter.get](e.2, &get)
  let u.4: unit = prim ev_set_clause[Counter.put](e.2, &put)
  let w.5: tobj = prim evv_push(w.0, e.2)
  let u.6: unit = prim evv_set(w.5)
  let e.7: obj = prim ev_lookup[Counter]()
  let c.8: tobj = prim ev_clause[Counter.put](e.7)
  let f.9: tobj = box 5
  let u.10: unit = clause Counter.put c.8(e.7, f.9)
  let e.11: obj = prim ev_lookup[Counter]()
  let c.12: tobj = prim ev_clause[Counter.get](e.11)
  let r.13: tobj = clause Counter.get c.12(e.11, ())
  let u.14: unit = prim evv_set(w.0)
  let n.15: int = unbox r.13
  decref r.13
  let t.16: obj = extern \"Prelude.Show Int.show\"(n.15)
  let v.17: unit = extern Prelude.println(t.16)
  return v.17
}
internal yields clause Counter.get fn get(e.0: obj, u.1: unit) -> tobj {
  let s.2: tobj = prim ev_take_state(e.0)
  dup s.2
  let v.3: unit = prim ev_set_state(e.0, s.2)
  decref e.0
  return s.2
}
internal yields clause Counter.put fn put(e.0: obj, n.1: tobj) -> unit {
  let u.2: unit = prim ev_set_state(e.0, n.1)
  decref e.0
  return u.2
}
";
    assert_eq!(run_both(text), "5\n");
}
