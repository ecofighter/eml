//! Miri で流す小さい実行のテスト (docs/implementation/testing.md の「Miri」)。VM は検証していないコードから `eml_rt`
//! の `unsafe fn` を呼ぶので、核の操作の前提を守っていることを確かめる。メモリの Host だけを使い、ファイルを開かない。
//! 確かめる道: 直接の呼び出しと戻り、`con` と `switch` と `release`、部分適用と余った引数、基本操作 (evidence を作って
//! 引く)、ヒープの文字列の extern、2つのプログラムを1つの `Machine` に続けて読み込み、2つ目の塊を付け替えること、
//! 文字列を返すエントリの結果を写すこと、`exit` の巻き戻し、`apply` と生の語の捕獲、複数の値の戻りと構造体と箱の間の
//! 変換、`par` のタスクの切り替えと子の中断、両方のモード。

use eml_vm::{AnyMachine, RunConfig, RuntimeError, Sched, VmFault, capture_host, execute};

fn run_both(text: &str) -> String {
    let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    run_program(&program, Sched::Sequential)
}

fn run_program(program: &eml_lir::Program, sched: Sched) -> String {
    run_counting_preemptions(program, sched).0
}

/// 両方のモードで流し、出力と、検査モードの横取りの数を返す。
fn run_counting_preemptions(program: &eml_lir::Program, sched: Sched) -> (String, u64) {
    let mut outputs = Vec::new();
    let mut preemptions = Vec::new();
    for debug_heap in [true, false] {
        let (host, out) = capture_host(Default::default());
        let config = RunConfig::default()
            .with_debug_heap(debug_heap)
            .with_sched(sched);
        let finished = execute(program, &config, host).unwrap_or_else(|error| panic!("{error}"));
        outputs.push(out.contents());
        preemptions.push(finished.preemptions);
    }
    assert_eq!(outputs[0], outputs[1]);
    (outputs.remove(0), preemptions[0])
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

/// 文字列を返すエントリの結果を写す (`Ctx::copy_string`)。`Finish` は結果を手放さず、写した後に手放す。
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
/// に渡す箱は、`same` の一様な変種が葉に開いて空け、`same` の葉の結果を同じ箱に入れ直す。`take` の一様な変種は箱を
/// 空けて葉を受け取り、`take` は構造体を葉のまま受ける。構造体はまだソースから作られないので、手書きの LIR を
/// パスに通した結果をそのまま書く。Miri の下でパスを流すと、実行よりも時間がかかるためである。テキストがパスの出力と
/// 同じであることは、`structs.rs` の `the_miri_texts_are_the_pass_output` が Miri の外で確かめる。
#[test]
fn multi_value_returns() {
    assert_eq!(run_both(MULTI_VALUE_RETURNS), "multi value\n1.5\n");
}

pub(crate) const MULTI_VALUE_RETURNS: &str = "\
layout Named { Named(obj, float) }
fn main() -> unit {
  let a.0: obj = const \"multi\"
  let b.1: obj = const \" value\"
  let s.2: obj = extern \"Prelude.++\"(a.0, b.1)
  let (p.0.11: obj, p.1.12: float) = call pair(s.2)
  let p.8: obj = con Named #0(p.0.11, p.1.12)
  let q.9: tobj = apply &same$uniform(p.8)
  let x.10: tobj = apply &take$uniform(q.9)
  let x.5: float = unbox x.10
  decref x.10
  let t.6: obj = extern \"Prelude.Show Float.show\"(x.5)
  let u.7: unit = extern Prelude.println(t.6)
  return u.7
}
fn pair(s.0: obj) -> (obj, float) {
  return (s.0, 1.5)
}
fn same(p.0.1: obj, p.1.2: float) -> (obj, float) {
  return (p.0.1, p.1.2)
}
fn take(p.0.4: obj, p.1.5: float) -> float {
  let u.3: unit = extern Prelude.println(p.0.4)
  return p.1.5
}
fn same$uniform(p.2: tobj) -> tobj {
  let (p.0.4: obj, p.1.5: float) = unbox p.2 Named
  let r.8: token = release unboxed p.2 Named #0(p.0.4, _)
  let (r.0.6: obj, r.1.7: float) = call same(p.0.4, p.1.5)
  let r.3: obj = con Named #0(r.0.6, r.1.7) reuse r.8 in place
  return r.3
}
fn take$uniform(p.2: tobj) -> tobj {
  let (p.0.4: obj, p.1.5: float) = unbox p.2 Named
  release unboxed p.2 Named #0(p.0.4, _)
  let r.1: float = call take(p.0.4, p.1.5)
  let r.3: tobj = box r.1
  return r.3
}
";

/// 共有の箱から、`tobj` の欄に入ったスカラーの葉と文字列の葉を2回取り出す (docs/spec/vm.md の「データとクロージャ」)。
/// 上の `multi_value_returns` と同じく、パスを通した結果を書く。
#[test]
fn shared_struct_boxes_with_scalar_leaves() {
    assert_eq!(
        run_both(SHARED_STRUCT_BOXES_WITH_SCALAR_LEAVES),
        "4611686018427387904\nhello\n"
    );
}

pub(crate) const SHARED_STRUCT_BOXES_WITH_SCALAR_LEAVES: &str = "\
layout (,,) { (,,)(tobj, tobj, tobj) }
fn main() -> unit {
  let w.0: tobj = apply &make$uniform(())
  dup w.0
  let x.6: tobj = apply &first$uniform(w.0)
  let x.1: int = unbox x.6
  decref x.6
  let y.2: obj = apply &third$uniform(w.0)
  let s.3: obj = extern \"Prelude.Show Int.show\"(x.1)
  let u.4: unit = extern Prelude.println(s.3)
  let u.5: unit = extern Prelude.println(y.2)
  return u.5
}
fn make(u.0: unit) -> (int, float, obj) {
  let a.1: obj = const \"he\"
  let b.2: obj = const \"llo\"
  let s.3: obj = extern \"Prelude.++\"(a.1, b.2)
  return (4611686018427387904, 2.5, s.3)
}
fn first(p.0.4: int, p.1.5: float, p.2.6: obj) -> int {
  decref p.2.6
  return p.0.4
}
fn third(p.0.4: int, p.1.5: float, p.2.6: obj) -> obj {
  return p.2.6
}
fn make$uniform(u.0: unit) -> tobj {
  let (r.0.3: int, r.1.4: float, r.2.5: obj) = call make(u.0)
  let r.6: tobj = box r.0.3
  let r.7: tobj = box r.1.4
  let r.2: obj = con (,,) #0(r.6, r.7, r.2.5)
  return r.2
}
fn first$uniform(p.2: tobj) -> tobj {
  let (p.0.4: tobj, p.1.6: tobj, p.2.8: obj) = unbox p.2 (,,)
  let p.0.5: int = unbox p.0.4
  let p.1.7: float = unbox p.1.6
  release unboxed p.2 (,,) #0(_, _, p.2.8)
  let r.1: int = call first(p.0.5, p.1.7, p.2.8)
  let r.3: tobj = box r.1
  return r.3
}
fn third$uniform(p.2: tobj) -> obj {
  let (p.0.3: tobj, p.1.5: tobj, p.2.7: obj) = unbox p.2 (,,)
  let p.0.4: int = unbox p.0.3
  let p.1.6: float = unbox p.1.5
  release unboxed p.2 (,,) #0(_, _, p.2.7)
  tail call third(p.0.4, p.1.6, p.2.7)
}
";

/// `par` の fork と join とタスクの切り替え。VM は検証していないコードから `share`、`transfer_back`、
/// `Ctx::switch_task` を呼び、`Task` と join の節点は切り替えをまたいでヒープの語を持つ。右の閉包は親も持つ組を
/// 捕まえるので、fork はそれに共有の印を付ける。右の子は新しい組を確保して返し、join がそれを親の局所に戻す。`Fail`
/// の evidence の下で fork するので、子の `evv` には鏡の fork evidence を作って印を付ける。横取りと join の両方で
/// 切り替わるよう、seed の方針で流す。Miri の時間を抑えるため、`par.rs` の前置きは使わず、要る関数だけを書く。子の
/// 中断は下の `a_child_abort_is_raised_again_after_the_join` が通す。
#[test]
fn par_switches_between_tasks() {
    let text = "\
layout (,) { (,)(tobj, tobj) }
effect Prelude.Panic { never panicked(obj) -> never }
effect Fail { never fail(int) -> never }
fn main() -> unit {
  let w.0: tobj = prim evv_get()
  dup w.0
  let ev.1: obj = prim ev_new[Fail]((), w.0)
  dup ev.1
  let w.2: tobj = prim evv_push(w.0, ev.1)
  let u.3: unit = prim evv_set(w.2)
  let a.4: obj = con (,) #0(#5, #6)
  dup a.4
  let g.5: tobj = closure swap(a.4)
  let t.6: obj = par (,) #0(&spin_one, g.5)
  let w.7: tobj = prim ev_take_evv(ev.1)
  let u.8: unit = prim evv_set(w.7)
  decref ev.1
  let (x.9: tobj, p.10: tobj) = unbox t.6 (,)
  dup x.9
  dup p.10
  decref t.6
  let (c.11: tobj, d.12: tobj) = unbox p.10 (,)
  dup c.11
  dup d.12
  decref p.10
  let (e.13: tobj, f.14: tobj) = unbox a.4 (,)
  dup e.13
  decref a.4
  let u.15: unit = call show(x.9)
  let u.16: unit = call show(c.11)
  let u.17: unit = call show(d.12)
  tail call show(e.13)
}
fn swap(a.0: obj, p.1: unit) -> tobj {
  let (x.2: tobj, y.3: tobj) = unbox a.0 (,)
  dup x.2
  dup y.3
  decref a.0
  let t.4: obj = con (,) #0(y.3, x.2)
  return t.4
}
fn show(t.0: tobj) -> unit {
  let n.1: int = unbox t.0
  decref t.0
  let s.2: obj = extern \"Prelude.Show Int.show\"(n.1)
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
clause Prelude.Panic.panicked fn fork$Prelude.Panic.panicked(ev.0: obj, a.1: obj) -> obj poly {
  let c.2: tobj = closure reperform$Prelude.Panic.panicked(a.1)
  let u.3: unit = prim child_abort(ev.0, c.2)
  decref ev.0
  let d.4: obj poly = prim dummy()
  return d.4
}
uniform from 1 fn reperform$Prelude.Panic.panicked(a.0: obj, u.1: unit) -> tobj poly {
  let r.2: tobj poly = extern Prelude.panic(a.0)
  return r.2
}
clause Fail.fail fn fork$Fail.fail(ev.0: obj, a.1: int) -> obj poly {
  let c.2: tobj = closure reperform$Fail.fail(a.1)
  let u.3: unit = prim child_abort(ev.0, c.2)
  decref ev.0
  let d.4: obj poly = prim dummy()
  return d.4
}
uniform from 1 fn reperform$Fail.fail(a.0: int, u.1: unit) -> tobj poly {
  let ev.2: obj = prim ev_lookup[Fail]()
  let c.3: tobj = prim ev_clause[Fail.fail](ev.2)
  tail clause Fail.fail c.3(ev.2, a.0)
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
";
    let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    let (out, preemptions) = run_counting_preemptions(&program, Sched::Seeded(1));
    assert_eq!(out, "1\n6\n5\n5\n");
    // スケジュールがずれて横取りが起きなくなると、横取りの切り替えを Miri で通らなくなる
    assert!(preemptions > 0, "Seeded(1) preempted {preemptions} times");
}

/// 右の子の `Panic` の中断。VM は `child_abort` で起こし直しの閉包を join の節点に記録し、子の ID の閉包を
/// `transfer_back` で親に戻してから、親の文脈で適用する。閉包はヒープの文字列を捕まえるので、移し戻しと
/// 適用がその物体を正しく扱うことを Miri で確かめる。捕まえる `try_panic` はないので、run は親で起こし直した
/// `panic` の誤りで終わる。
#[test]
fn a_child_abort_is_raised_again_after_the_join() {
    let text = "\
layout (,) { (,)(tobj, tobj) }
effect Prelude.Panic { never panicked(obj) -> never }
fn main() -> unit {
  let t.0: obj = par (,) #0(&one, &boom)
  decref t.0
  return ()
}
fn one(p.0: unit) -> tobj {
  let n.1: tobj = box 1
  return n.1
}
fn boom(p.0: unit) -> tobj {
  let m.1: obj = extern \"Prelude.Show Int.show\"(42)
  let r.2: tobj = extern Prelude.panic(m.1)
  return r.2
}
clause Prelude.Panic.panicked fn fork$Prelude.Panic.panicked(ev.0: obj, a.1: obj) -> obj poly {
  let c.2: tobj = closure reperform$Prelude.Panic.panicked(a.1)
  let u.3: unit = prim child_abort(ev.0, c.2)
  decref ev.0
  let d.4: obj poly = prim dummy()
  return d.4
}
uniform from 1 fn reperform$Prelude.Panic.panicked(a.0: obj, u.1: unit) -> tobj poly {
  let r.2: tobj poly = extern Prelude.panic(a.0)
  return r.2
}
";
    let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    for debug_heap in [true, false] {
        let (host, out) = capture_host(Default::default());
        let config = RunConfig::default().with_debug_heap(debug_heap);
        let error = execute(&program, &config, host).expect_err("the panic is not caught");
        assert_eq!(out.contents(), "", "debug_heap: {debug_heap}");
        // 文言は `par.rs` が固定するので、ここでは起こし直した `panic` で終わったことだけを見る
        assert!(
            matches!(
                error,
                RuntimeError::Fault {
                    fault: VmFault::Panicked(_),
                    ..
                }
            ),
            "debug_heap: {debug_heap}: {error}"
        );
    }
}
