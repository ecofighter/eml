//! Miri で流す小さい実行のテスト (docs/implementation/testing.md の「Miri」)。VM は検証していないコードから `eml_rt`
//! の `unsafe fn` を呼ぶので、核の操作の前提を守っていることを確かめる。メモリの Host だけを使い、ファイルを開かない。
//! 確かめる道: 直接の呼び出しと戻り、`con` と `switch` と `release`、部分適用と余った引数、基本操作 (evidence を作って
//! 引く)、ヒープの文字列の extern、2つのプログラムを1つの `Machine` に続けて読み込み、2つ目の塊を付け替えること、
//! 文字列を返す入口の結果を写すこと、`exit` の巻き戻し、`apply` と生の語の捕獲、複数の値の戻りと構造体と箱の間の
//! 変換、`par` のタスクの切り替え、両方のモード。

use eml_vm::{AnyMachine, RunConfig, Sched, capture_host, execute};

fn run_both(text: &str) -> String {
    let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    run_program(&program, Sched::Sequential)
}

fn run_program(program: &eml_lir::Program, sched: Sched) -> String {
    let mut outputs = Vec::new();
    for debug_heap in [true, false] {
        let (host, out) = capture_host(Default::default());
        let config = RunConfig::default()
            .with_debug_heap(debug_heap)
            .with_sched(sched);
        execute(program, &config, host).unwrap_or_else(|error| panic!("{error}"));
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
  let c.2: int = extern \"Prelude.Num Int.+\"(a.0, b.1)
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

/// 2つ目のプログラムの新しい文字列は新しい塊に置き、実行のヒープはその塊に付け替える。1つ目の塊の文字列も読み続ける。
#[test]
fn a_second_load_attaches_a_new_chunk() {
    let first = "\
fn main() -> unit {
  let s.0: obj = const \"one\"
  let u.1: unit = extern Prelude.println(s.0)
  return u.1
}
";
    let second = "\
fn second() -> unit {
  let s.0: obj = const \"one\"
  let t.1: obj = const \"two\"
  let u.2: obj = extern \"Prelude.++\"(s.0, t.1)
  let v.3: unit = extern Prelude.println(u.2)
  return v.3
}
";
    for debug_heap in [true, false] {
        let (host, out) = capture_host(Default::default());
        let config = RunConfig::default().with_debug_heap(debug_heap);
        let mut machine = AnyMachine::new(&config, host);
        for text in [first, second] {
            let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
            eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
            let loaded = machine
                .load(&program)
                .unwrap_or_else(|error| panic!("{error:?}"));
            machine
                .run(loaded.entry)
                .unwrap_or_else(|error| panic!("{error}"));
        }
        machine.close().unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(out.contents(), "one\nonetwo\n");
    }
}

/// 文字列を返す入口の結果を写す (`Ctx::copy_string`)。`Finish` は結果を手放さず、写した後に手放す。
#[test]
fn a_shown_entry_copies_its_result() {
    let text = "\
fn shown() -> obj {
  let s.0: obj = const \"é\"
  let n.1: obj = extern \"Prelude.Show Int.show\"(7)
  let t.2: obj = extern \"Prelude.++\"(s.0, n.1)
  return t.2
}
";
    let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    for debug_heap in [true, false] {
        let (host, _) = capture_host(Default::default());
        let config = RunConfig::default().with_debug_heap(debug_heap);
        let mut machine = AnyMachine::new(&config, host);
        let loaded = machine.load(&program).unwrap();
        let (_, shown) = machine.run_shown(loaded.entry, 10).unwrap();
        assert_eq!(shown.as_deref(), Some("é7"));
        machine.close().unwrap_or_else(|error| panic!("{error}"));
    }
}

/// `exit` は、文字列を持つフレームと、余った引数を預けた `ApplyRest` の枠を巻き戻して `Finish` に届く。どの文字列も
/// 巻き戻しで1回ずつ手放す (docs/spec/vm.md の「巻き戻しの手順」)。
#[test]
fn exit_unwinds_frames_holding_strings() {
    let text = "\
fn main() -> unit {
  let a.0: obj = const \"main\"
  let b.1: obj = const \"!\"
  dup b.1
  let s.2: obj = extern \"Prelude.++\"(a.0, b.1)
  let c.3: obj = const \"rest\"
  let d.4: obj = extern \"Prelude.++\"(c.3, b.1)
  let n.5: tobj = box 5
  let r.6: tobj = apply &mid(n.5, d.4)
  decref r.6
  let u.7: unit = extern Prelude.println(s.2)
  return u.7
}
fn mid(n.0: tobj) -> tobj {
  let m.1: int = unbox n.0
  decref n.0
  let s.2: obj = extern \"Prelude.Show Int.show\"(m.1)
  let z.3: unit = call bottom(m.1)
  return s.2
}
fn bottom(n.0: int) -> unit {
  let z.1: unit = extern Prelude.exit(n.0)
  return z.1
}
";
    let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    for debug_heap in [true, false] {
        let (host, out) = capture_host(Default::default());
        let config = RunConfig::default().with_debug_heap(debug_heap);
        let finished = execute(&program, &config, host).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!((finished.exit, out.contents()), (Some(5), String::new()));
        // `bottom`、`mid`、`ApplyRest` の枠、`main` の4つ。数は検査モードだけで数える
        let unwound = if debug_heap { 4 } else { 0 };
        assert_eq!(finished.stats.unwound_frames, unwound);
    }
}

/// 捕まえた `float` を生の語に書き、`apply` で引数の順に読み出す (`make_closure` と `take_captures`)。後から足す
/// 引数と結果は一様な箱である。
#[test]
fn an_apply_with_raw_captures() {
    let text = "\
fn main() -> unit {
  let c.0: tobj = closure scale(2.5)
  let b.1: tobj = box 4.0
  let r.2: tobj = apply c.0(b.1)
  let x.3: float = unbox r.2
  decref r.2
  let s.4: obj = extern \"Prelude.Show Float.show\"(x.3)
  let u.5: unit = extern Prelude.println(s.4)
  return u.5
}
fn scale(k.0: float, x.1: tobj) -> tobj {
  let y.2: float = unbox x.1
  decref x.1
  let z.3: float = extern \"Prelude.Num Float.*\"(k.0, y.2)
  let r.4: tobj = box z.3
  return r.4
}
";
    assert_eq!(run_both(text), "10.0\n");
}

/// 複数の値の戻りと、構造体と箱の間の変換。構造体は参照の葉 (文字列) を持つ。`pair` は葉を `RetN` で返す。`apply`
/// に渡す箱は、`same` の一様な版が葉に開いて空け、`same` の葉の結果を版が箱に入れる。`main` の
/// 構造体の `unbox` はその箱から葉を借り、`release unboxed` が箱を空けて葉を受け取る。`take` は構造体を葉のまま
/// 受ける。構造体はまだソースから作られないので、手書きの LIR をパスに通す。
#[test]
fn multi_value_returns() {
    let named = "Named{obj, float}";
    let text = format!(
        "\
layout Named {{ Named(obj, float) }}
fn main() -> unit {{
  let a.0: obj = const \"multi\"
  let b.1: obj = const \" value\"
  let s.2: obj = extern \"Prelude.++\"(a.0, b.1)
  let p.3: {named} = call pair(s.2)
  let q.4: {named} = apply &same(p.3)
  let x.5: float = apply &take(q.4)
  let t.6: obj = extern \"Prelude.Show Float.show\"(x.5)
  let u.7: unit = extern Prelude.println(t.6)
  return u.7
}}
fn pair(s.0: obj) -> {named} {{
  let p.1: {named} = con Named #0(s.0, 1.5)
  return p.1
}}
fn same(p.0: {named}) -> {named} {{
  return p.0
}}
fn take(p.0: {named}) -> float {{
  unpack p.0 Named #0(s.1: obj, x.2: float)
  let u.3: unit = extern Prelude.println(s.1)
  return x.2
}}
"
    );
    let mut program = eml_lir::parse(&text).unwrap_or_else(|error| panic!("{error}"));
    eml_lir::passes_until(&mut program, eml_lir::Pass::Reuse);
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        run_program(&program, Sched::Sequential),
        "multi value\n1.5\n"
    );
}

/// 共有の箱から、`tobj` の欄に入ったスカラーの葉と文字列の葉を2回取り出す (docs/spec/vm.md の「データとクロージャ」)。
#[test]
fn shared_struct_boxes_with_scalar_leaves() {
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
    let mut program = eml_lir::parse(&text).unwrap_or_else(|error| panic!("{error}"));
    eml_lir::passes_until(&mut program, eml_lir::Pass::Reuse);
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        run_program(&program, Sched::Sequential),
        "4611686018427387904\nhello\n"
    );
}

/// `par` のタスクの切り替え。`Task` と join の節点は切り替えをまたいでヒープの語を持ち、VM は検証していないコードから
/// `share`、`transfer_back`、`Ctx::switch_task` を呼ぶ。2つの子は確保した値を返す。横取りと join の両方で切り替わる
/// よう、seed の方針で流す。Miri の時間を抑えるため、`par.rs` の前置きは使わず、要る fork の関数だけを書く。子の
/// 中断は `par.rs` が確かめる。
#[test]
fn par_switches_between_tasks() {
    let text = "\
layout (,) { (,)(tobj, tobj) }
effect Prelude.Panic { never panicked(obj) -> never }
fn main() -> unit {
  let t.0: obj = par (,) #0(&spin_one, &spin_two)
  let (a.1: tobj, b.2: tobj) = unbox t.0 (,)
  let m.3: int = unbox a.1
  let n.4: int = unbox b.2
  decref t.0
  let s.5: obj = extern \"Prelude.Show Int.show\"(m.3)
  let u.6: unit = extern Prelude.println(s.5)
  let s.7: obj = extern \"Prelude.Show Int.show\"(n.4)
  let u.8: unit = extern Prelude.println(s.7)
  return u.8
}
clause Prelude.Panic.panicked fn fork$Prelude.Panic.panicked(ev.0: obj, a.1: obj) -> obj poly {
  let u.2: unit = prim child_abort(ev.0, &reperform$Prelude.Panic.panicked, a.1)
  decref ev.0
  let d.3: obj poly = prim dummy()
  return d.3
}
fn reperform$Prelude.Panic.panicked(t.0: obj poly) -> tobj poly {
  let r.1: tobj poly = extern Prelude.panic(t.0)
  return r.1
}
fn spin(n.0: int) -> unit {
  switch n.0 { 0 -> b1, _ -> b2 }
b1:
  return ()
b2:
  let m.1: int = extern \"Prelude.Num Int.-\"(n.0, 1)
  let u.2: unit = call spin(m.1)
  return u.2
}
fn spin_one(p.0: unit) -> tobj {
  let u.1: unit = call spin(8)
  let n.2: tobj = box 1
  return n.2
}
fn spin_two(p.0: unit) -> tobj {
  let u.1: unit = call spin(8)
  let n.2: tobj = box 2
  return n.2
}
";
    let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(run_program(&program, Sched::Seeded(1)), "1\n2\n");
}
