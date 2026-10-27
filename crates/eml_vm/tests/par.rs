//! `par` の fork と join を、evidence passing の出力の形の手書きの LIR で確かめる (docs/spec/vm.md の
//! 「`par` の fork と join」)。fork の関数 (`fork$E.op` と `reperform$E.op`) は evidence passing が作る形で書く
//! (docs/spec/core-ir.md の「`par` の子の中断」)。各プログラムは検査モードで逐次と2つの seed で流して結果を比べ、
//! 通れば速いモードの逐次でも流す。子の誤りの規則は、ソースから作ったコードでも確かめる (`source`)。

use std::sync::Arc;

use eml_rt::{Fault, HeapError};
use eml_vm::{Control, Finished, Limits, RunConfig, RuntimeError, Sched, VmFault};

use crate::common::parse;

/// 組の構築子、`Panic` と `Fail` のエフェクト、それぞれの fork の関数、`prompt$` とその補助の関数。
const PRELUDE: &str = "\
layout (,) { (,)(tobj, tobj) }
effect Prelude.Panic { never panicked(obj) -> never }
effect Fail { never fail(int) -> never }
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
yields fn prompt$(ev.0: obj, ret.1: tobj poly, x.2: tobj poly) -> tobj poly {
  let w.3: tobj = prim ev_take_evv(ev.0)
  let u.4: unit = prim evv_set(w.3)
  let s.5: int = prim yield_status(ev.0)
  switch s.5 { 0 -> b1, 1 -> b2, 2 -> b3, _ -> b4 }
b1:
  let st.6: tobj poly = prim ev_take_state(ev.0)
  decref ev.0
  tail yields apply ret.1(x.2, st.6)
b2:
  let u.8: unit = drop x.2
  let k.9: tobj poly = prim yield_cont_take()
  let c.10: tobj poly = prim yield_finish()
  let st.11: tobj poly = prim ev_take_state(ev.0)
  let k.12: tobj poly = closure resume$(ev.0, ret.1, k.9)
  tail yields apply c.10(k.12, st.11)
b3:
  let u.14: unit = drop ret.1
  let u.15: unit = drop x.2
  let c.16: tobj poly = prim yield_finish()
  let st.17: tobj poly = prim ev_take_state(ev.0)
  decref ev.0
  tail yields apply c.16(st.17)
b4:
  let u.19: unit = drop x.2
  let st.20: tobj poly = prim ev_take_state(ev.0)
  let k.21: tobj poly = prim yield_cont_take()
  let k.22: tobj poly = closure rewind$(ev.0, ret.1, st.20, k.21)
  let u.23: unit = prim yield_cont_set(k.22)
  let d.24: obj poly = prim dummy()
  return d.24
}
yields fn resume$(ev.0: obj, ret.1: tobj poly, k.2: tobj poly, v.3: tobj poly, s.4: tobj poly) -> tobj poly {
  let e.5: obj = prim ev_renew(ev.0, s.4)
  decref ev.0
  let w.6: tobj = prim evv_get()
  dup e.5
  let w.7: tobj = prim evv_push(w.6, e.5)
  let u.8: unit = prim evv_set(w.7)
  let x.9: tobj poly = yields apply k.2(v.3) prompt e.5
  tail call prompt$(e.5, ret.1, x.9)
}
yields fn rewind$(ev.0: obj, ret.1: tobj poly, s.2: tobj poly, k.3: tobj poly, v.4: tobj poly) -> tobj poly {
  tail call resume$(ev.0, ret.1, k.3, v.4, s.2)
}
fn return$(r.0: tobj, s.1: unit) -> tobj {
  return r.0
}
fn show_pair(t.0: tobj) -> unit {
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
fn one(p.0: unit) -> tobj {
  let n.1: tobj = box 1
  return n.1
}
fn two(p.0: unit) -> tobj {
  let n.1: tobj = box 2
  return n.1
}
fn fail7(p.0: unit) -> tobj {
  let ev.1: obj = prim ev_lookup[Fail]()
  let c.2: tobj = prim ev_clause[Fail.fail](ev.1)
  let t.3: tobj = clause Fail.fail c.2(ev.1, 7)
  return t.3
}
fn fail8(p.0: unit) -> tobj {
  let ev.1: obj = prim ev_lookup[Fail]()
  let c.2: tobj = prim ev_clause[Fail.fail](ev.1)
  let t.3: tobj = clause Fail.fail c.2(ev.1, 8)
  return t.3
}
fn with_outer(f.0: tobj) -> tobj {
  let w.1: tobj = prim evv_get()
  dup w.1
  let ev.2: obj = prim ev_new[Fail]((), w.1)
  let u.3: unit = prim ev_set_clause[Fail.fail](ev.2, &outer$entry)
  dup ev.2
  let w.4: tobj = prim evv_push(w.1, ev.2)
  let u.5: unit = prim evv_set(w.4)
  let x.6: tobj poly = yields apply f.0(()) prompt ev.2
  tail call prompt$(ev.2, &return$, x.6)
}
clause Fail.fail fn outer$entry(e.0: obj, n.1: int) -> obj poly {
  let n.2: tobj = box n.1
  let c.3: tobj = closure outer$clause(n.2)
  let u.4: unit = prim yield_abort(e.0, c.3)
  let d.5: obj poly = prim dummy()
  return d.5
}
fn outer$clause(n.0: tobj, s.1: unit) -> tobj {
  let h.2: tobj = box 100
  let t.3: obj = con (,) #0(n.0, h.2)
  return t.3
}
fn with_inner(f.0: tobj) -> tobj {
  let w.1: tobj = prim evv_get()
  dup w.1
  let ev.2: obj = prim ev_new[Fail]((), w.1)
  let u.3: unit = prim ev_set_clause[Fail.fail](ev.2, &inner$entry)
  dup ev.2
  let w.4: tobj = prim evv_push(w.1, ev.2)
  let u.5: unit = prim evv_set(w.4)
  let x.6: tobj poly = yields apply f.0(()) prompt ev.2
  tail call prompt$(ev.2, &return$, x.6)
}
clause Fail.fail fn inner$entry(e.0: obj, n.1: int) -> obj poly {
  let n.2: tobj = box n.1
  let c.3: tobj = closure inner$clause(n.2)
  let u.4: unit = prim yield_abort(e.0, c.3)
  let d.5: obj poly = prim dummy()
  return d.5
}
fn inner$clause(n.0: tobj, s.1: unit) -> tobj {
  let h.2: tobj = box 200
  let t.3: obj = con (,) #0(n.0, h.2)
  return t.3
}
fn try_panic(f.0: tobj, c.1: tobj) -> tobj {
  let w.2: tobj = prim evv_get()
  dup w.2
  let ev.3: obj = prim ev_new[Prelude.Panic](c.1, w.2)
  let u.4: unit = prim ev_set_clause[Prelude.Panic.panicked](ev.3, &panicked$entry)
  dup ev.3
  let w.5: tobj = prim evv_push(w.2, ev.3)
  let u.6: unit = prim evv_set(w.5)
  let x.7: tobj poly = yields apply f.0(()) prompt ev.3
  tail call prompt$(ev.3, &return$, x.7)
}
clause Prelude.Panic.panicked fn panicked$entry(e.0: obj, m.1: obj) -> obj poly {
  decref m.1
  let u.2: unit = prim yield_abort(e.0, &panicked$clause)
  let d.3: obj poly = prim dummy()
  return d.3
}
fn panicked$clause(c.0: tobj) -> tobj {
  return c.0
}
fn boom(p.0: unit) -> tobj {
  let m.1: obj = const \"boom\"
  let r.2: tobj = extern Prelude.panic(m.1)
  return r.2
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
  let u.1: unit = call spin(40)
  let n.2: tobj = box 1
  return n.2
}
fn spin_two(p.0: unit) -> tobj {
  let u.1: unit = call spin(40)
  let n.2: tobj = box 2
  return n.2
}
";

/// 手書きの LIR を verifier に通し、検査モードで逐次、`Seeded(1)`、`Seeded(2026)` の順に流す。どの方針でも出力、
/// 誤りの文言、スケジュールによらない `RunStats` が同じでなければならない。逐次が通れば、速いモードの逐次でも流して
/// 出力を比べる。逐次の検査モードの出力と結果を返す。
fn run_par_lir(text: &str, max_depth: usize) -> (String, Result<Finished, RuntimeError>) {
    let program = parse(text);
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}\n{text}"));
    run_par_program(&program, max_depth)
}

/// `run_par_lir` の、読んで確かめた後の部分。
fn run_par_program(
    program: &eml_lir::Program,
    max_depth: usize,
) -> (String, Result<Finished, RuntimeError>) {
    let (out, result) = run_in(program, true, Sched::Sequential, max_depth);
    for seed in [1, 2026] {
        let (seeded_out, seeded) = run_in(program, true, Sched::Seeded(seed), max_depth);
        assert_eq!(seeded_out, out, "the output under seed {seed}");
        assert_eq!(
            summary(&seeded),
            summary(&result),
            "the result under seed {seed}"
        );
    }
    if result.is_ok() {
        let (fast_out, fast) = run_in(program, false, Sched::Sequential, max_depth);
        assert_eq!(fast.map(|_| ()), Ok(()), "the fast mode failed");
        assert_eq!(fast_out, out, "the fast mode printed something else");
    }
    (out, result)
}

/// 方針によらない結果。誤りは文言で、成功はスケジュールによらない `RunStats` で比べる。
fn summary(result: &Result<Finished, RuntimeError>) -> Result<eml_vm::RunStats, String> {
    match result {
        Ok(finished) => Ok(eml_test_support::schedule_independent(finished.stats)),
        Err(error) => Err(error.to_string()),
    }
}

fn run_in(
    program: &eml_lir::Program,
    debug_heap: bool,
    sched: Sched,
    max_depth: usize,
) -> (String, Result<Finished, RuntimeError>) {
    let control = Arc::new(Control::new(Limits {
        max_depth,
        ..Limits::default()
    }));
    run_with(program, debug_heap, sched, control)
}

fn run_with(
    program: &eml_lir::Program,
    debug_heap: bool,
    sched: Sched,
    control: Arc<Control>,
) -> (String, Result<Finished, RuntimeError>) {
    let (host, out) = eml_vm::capture_host(Default::default());
    let config = RunConfig::default()
        .with_debug_heap(debug_heap)
        .with_control(control)
        .with_sched(sched);
    let result = eml_vm::execute(program, &config, host);
    (out.contents(), result)
}

/// 入口は最初の関数なので、`PRELUDE` の前に置く。レイアウトとエフェクトの宣言は先頭に要る。
fn with_prelude(text: &str) -> String {
    let (decls, functions) =
        PRELUDE.split_at(PRELUDE.find("clause ").expect("the prelude has functions"));
    format!("{decls}{text}{functions}")
}

fn run_par(text: &str) -> (String, Result<Finished, RuntimeError>) {
    run_par_lir(&with_prelude(text), usize::MAX)
}

/// 両方の子が値を返すと、join は2つの値の組を作る。
#[test]
fn both_children_return_values() {
    let (out, result) = run_par(
        "\
fn main() -> unit {
  let t.0: obj = par (,) #0(&one, &two)
  tail call show_pair(t.0)
}
",
    );
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "1\n2\n");
}

/// 左の子の中断は、両方の子が終わった後で親の handler に起こし直す。右の子の値は捨てる。
#[test]
fn a_left_abort_is_raised_again_in_the_parent_handler() {
    let (out, result) = run_par(
        "\
fn main() -> unit {
  let t.0: tobj = call with_outer(&body)
  tail call show_pair(t.0)
}
fn body(p.0: unit) -> tobj {
  let t.1: obj = par (,) #0(&fail7, &two)
  return t.1
}
",
    );
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "7\n100\n");
}

/// 左の子が値を返し、右の子が中断すると、左の値を捨てて右の中断を起こし直す。
#[test]
fn a_right_abort_drops_the_left_value() {
    let (out, result) = run_par(
        "\
fn main() -> unit {
  let t.0: tobj = call with_outer(&body)
  tail call show_pair(t.0)
}
fn body(p.0: unit) -> tobj {
  let t.1: obj = par (,) #0(&one, &fail7)
  return t.1
}
",
    );
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "7\n100\n");
}

/// 両方の子が中断すれば、いちばん左の中断を起こし直す。右の子も最後まで走り、その中断は捨てる。
#[test]
fn the_leftmost_abort_wins() {
    let (out, result) = run_par(
        "\
fn main() -> unit {
  let t.0: tobj = call with_outer(&body)
  tail call show_pair(t.0)
}
fn body(p.0: unit) -> tobj {
  let t.1: obj = par (,) #0(&fail8, &fail7)
  return t.1
}
",
    );
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "8\n100\n");
}

/// mask を持つ `par` は evidence passing の `mask` の包みの中にある。子の `evv` は mask した後の親の `evv` の鏡なので、
/// 子の中断は内側の handler を飛ばして外側の handler に届く。
#[test]
fn a_masked_par_raises_past_the_inner_handler() {
    let (out, result) = run_par(
        "\
fn main() -> unit {
  let t.0: tobj = call with_outer(&middle)
  tail call show_pair(t.0)
}
fn middle(p.0: unit) -> tobj {
  tail call with_inner(&body)
}
fn body(p.0: unit) -> tobj {
  let w.1: tobj = prim evv_get()
  let w.2: tobj = prim evv_mask[Fail](w.1)
  let u.3: unit = prim evv_set(w.2)
  let t.4: obj = par (,) #0(&fail7, &two)
  let u.5: unit = prim evv_set(w.1)
  return t.4
}
",
    );
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "7\n100\n");
}

/// 子の中の `mask` は、親の列と同じ形の fork evidence を数で飛ばす。2つ目の `Fail` の fork evidence は深さ 1 を持ち、
/// 親は mask した `evv` で起こし直すので、外側の handler に届く。左の子でも右の子でも同じである。
#[test]
fn a_mask_inside_a_child_reaches_the_second_parent_handler() {
    for (left, right) in [("skip7", "two"), ("one", "skip7")] {
        let (out, result) = run_par(&format!(
            "\
fn main() -> unit {{
  let t.0: tobj = call with_outer(&middle)
  tail call show_pair(t.0)
}}
fn middle(p.0: unit) -> tobj {{
  tail call with_inner(&body)
}}
fn body(p.0: unit) -> tobj {{
  let t.1: obj = par (,) #0(&{left}, &{right})
  return t.1
}}
fn skip7(p.0: unit) -> tobj {{
  let w.1: tobj = prim evv_get()
  let w.2: tobj = prim evv_mask[Fail](w.1)
  let u.3: unit = prim evv_set(w.2)
  let t.4: tobj = call fail7(())
  let u.5: unit = prim evv_set(w.1)
  return t.4
}}
"
        ));
        assert!(result.is_ok(), "{left}, {right}: {result:?}");
        assert_eq!(out, "7\n100\n", "{left}, {right}");
    }
}

/// 子の中の `try_panic` は、子の中の `panic` を捕まえる。fork evidence の `Panic` より内側にあるためである。
#[test]
fn a_child_panic_is_caught_by_the_child_try_panic() {
    for (left, right, expected) in [("catches", "two", "42\n2\n"), ("one", "catches", "1\n42\n")] {
        let (out, result) = run_par(&format!(
            "\
fn main() -> unit {{
  let t.0: obj = par (,) #0(&{left}, &{right})
  tail call show_pair(t.0)
}}
fn catches(p.0: unit) -> tobj {{
  let c.1: tobj = box 42
  tail call try_panic(&boom, c.1)
}}
"
        ));
        assert!(result.is_ok(), "{left}, {right}: {result:?}");
        assert_eq!(out, expected, "{left}, {right}");
    }
}

/// 子の中で捕まえられない `panic` は、末尾の `Panic` の fork evidence から親で起こし直し、親の `try_panic` に届く。
#[test]
fn a_child_panic_is_caught_by_the_parent_try_panic() {
    for (left, right) in [("boom", "two"), ("one", "boom")] {
        let (out, result) = run_par(&format!(
            "\
fn main() -> unit {{
  let c.0: tobj = box 3
  let d.1: tobj = box 4
  let p.2: obj = con (,) #0(c.0, d.1)
  let t.3: tobj = call try_panic(&body, p.2)
  tail call show_pair(t.3)
}}
fn body(p.0: unit) -> tobj {{
  let t.1: obj = par (,) #0(&{left}, &{right})
  return t.1
}}
"
        ));
        assert!(result.is_ok(), "{left}, {right}: {result:?}");
        assert_eq!(out, "3\n4\n", "{left}, {right}");
    }
}

/// どこでも捕まえられない子の `panic` は、起こし直した `reperform$` の中の実行時エラーになり、ソースの位置を持たない。
#[test]
fn an_uncaught_child_panic_names_the_reperform() {
    let (out, result) = run_par(
        "\
fn main() -> unit {
  let t.0: obj = par (,) #0(&one, &boom)
  tail call show_pair(t.0)
}
",
    );
    assert_eq!(out, "");
    assert_eq!(
        error_text(result),
        "panicked: boom in `reperform$Prelude.Panic.panicked`"
    );
}

/// 入れ子の `par`。左の子の左の子は親のスタックの上で、右の子の右の子は右の子のタスクから作るタスクで動く。
#[test]
fn nested_pars_return_nested_pairs() {
    let (out, result) = run_par(
        "\
fn main() -> unit {
  let t.0: obj = par (,) #0(&left, &right)
  let (a.1: tobj, b.2: tobj) = unbox t.0 (,)
  dup a.1
  dup b.2
  decref t.0
  let u.3: unit = call show_pair(a.1)
  tail call show_pair(b.2)
}
fn left(p.0: unit) -> tobj {
  let t.1: obj = par (,) #0(&one, &spin_two)
  return t.1
}
fn right(p.0: unit) -> tobj {
  let t.1: obj = par (,) #0(&spin_two, &one)
  return t.1
}
",
    );
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "1\n2\n2\n1\n");
}

/// 入れ子の `par` の中の中断。右の子の右の子の中断は、2つの join を通って外側の handler に届く。左の子の中の `par`
/// の中断も同じである。
#[test]
fn an_abort_in_a_nested_par_crosses_both_joins() {
    for (left, right) in [("one", "nested"), ("nested", "two")] {
        let (out, result) = run_par(&format!(
            "\
fn main() -> unit {{
  let t.0: tobj = call with_outer(&body)
  tail call show_pair(t.0)
}}
fn body(p.0: unit) -> tobj {{
  let t.1: obj = par (,) #0(&{left}, &{right})
  return t.1
}}
fn nested(p.0: unit) -> tobj {{
  let t.1: obj = par (,) #0(&spin_two, &fail8)
  return t.1
}}
"
        ));
        assert!(result.is_ok(), "{left}, {right}: {result:?}");
        assert_eq!(out, "8\n100\n", "{left}, {right}");
    }
}

/// 深さの上限で止まる子の関数と、出力してから値を返す子の関数。
const DEEP: &str = "\
fn deep_left(p.0: unit) -> tobj {
  let t.1: tobj = call deep_left(())
  return t.1
}
fn deep_right(p.0: unit) -> tobj {
  let t.1: tobj = call deep_right(())
  return t.1
}
fn noisy(p.0: unit) -> tobj {
  let s.1: obj = const \"left\"
  let u.2: unit = extern Prelude.println(s.1)
  let n.3: tobj = box 1
  return n.3
}
";

fn run_deep(main: &str) -> (String, Result<Finished, RuntimeError>) {
    run_par_lir(&with_prelude(&format!("{main}{DEEP}")), 1000)
}

fn error_text(result: Result<Finished, RuntimeError>) -> String {
    result.map(|_| ()).unwrap_err().to_string()
}

/// 右の子の誤りは join に記録し、左の子が終わった後で報告する。左の子は最後まで走る。
#[test]
fn a_right_depth_error_is_reported_after_the_left_finishes() {
    let (out, result) = run_deep(
        "\
fn main() -> unit {
  let t.0: obj = par (,) #0(&noisy, &deep_right)
  tail call show_pair(t.0)
}
",
    );
    assert_eq!(out, "left\n");
    assert_eq!(
        error_text(result),
        "the call depth limit is exceeded in `deep_right`"
    );
}

/// 左の子の誤りはすぐに評価を止める。
#[test]
fn a_left_depth_error_stops_at_once() {
    let (out, result) = run_deep(
        "\
fn main() -> unit {
  let t.0: obj = par (,) #0(&deep_left, &two)
  tail call show_pair(t.0)
}
",
    );
    assert_eq!(out, "");
    assert_eq!(
        error_text(result),
        "the call depth limit is exceeded in `deep_left`"
    );
}

/// 子の誤りは、左の子の中断では救わない。
#[test]
fn a_left_abort_does_not_rescue_a_right_error() {
    let (out, result) = run_deep(
        "\
fn main() -> unit {
  let t.0: tobj = call with_outer(&body)
  tail call show_pair(t.0)
}
fn body(p.0: unit) -> tobj {
  let t.1: obj = par (,) #0(&fail7, &deep_right)
  return t.1
}
",
    );
    assert_eq!(out, "");
    assert_eq!(
        error_text(result),
        "the call depth limit is exceeded in `deep_right`"
    );
}

/// 両方の子が誤りで止まれば、左の誤りを報告する。
#[test]
fn the_left_error_wins_when_both_children_fail() {
    let (out, result) = run_deep(
        "\
fn main() -> unit {
  let t.0: obj = par (,) #0(&deep_left, &deep_right)
  tail call show_pair(t.0)
}
",
    );
    assert_eq!(out, "");
    assert_eq!(
        error_text(result),
        "the call depth limit is exceeded in `deep_left`"
    );
}

/// 右の子の中の `par` の左の子の誤りは、右の子の誤りになる。右の子が作った右の子は走らせずに捨てる。
#[test]
fn a_right_child_error_drops_its_own_children() {
    let (out, result) = run_deep(
        "\
fn main() -> unit {
  let t.0: obj = par (,) #0(&noisy, &right)
  tail call show_pair(t.0)
}
fn right(p.0: unit) -> tobj {
  let t.1: obj = par (,) #0(&deep_right, &spin_two)
  return t.1
}
",
    );
    assert_eq!(out, "left\n");
    assert_eq!(
        error_text(result),
        "the call depth limit is exceeded in `deep_right`"
    );
}

/// 右の子の深さは fork のときの親の深さから数えるので、`par` を通る再帰はどちらの子の側でも深さの上限で止まる。
#[test]
fn recursion_through_par_hits_the_depth_limit() {
    for (left, right) in [("again", "two"), ("one", "again")] {
        let (_, result) = run_deep(&format!(
            "\
fn main() -> unit {{
  let t.0: tobj = call again(())
  tail call show_pair(t.0)
}}
fn again(p.0: unit) -> tobj {{
  let t.1: obj = par (,) #0(&{left}, &{right})
  return t.1
}}
"
        ));
        assert_eq!(
            error_text(result),
            "the call depth limit is exceeded in `again`",
            "{left}, {right}"
        );
    }
}

/// 割り込みは子の中でもすぐに実行を止める。`par` の後の安全点で、右の子が始まる前に止まる。
#[test]
fn an_interrupt_stops_the_run_before_the_right_child_runs() {
    let program = parse(&with_prelude(
        "\
fn main() -> unit {
  let t.0: obj = par (,) #0(&one, &noisy)
  tail call show_pair(t.0)
}
fn noisy(p.0: unit) -> tobj {
  let s.1: obj = const \"right\"
  let u.2: unit = extern Prelude.println(s.1)
  let n.3: tobj = box 2
  return n.3
}
",
    ));
    for sched in [Sched::Sequential, Sched::Seeded(1), Sched::Seeded(2026)] {
        let control = Arc::new(Control::new(Limits::default()));
        control.interrupt();
        let (out, result) = run_with(&program, true, sched, control);
        assert_eq!(out, "", "{sched:?}");
        assert!(
            result.as_ref().is_err_and(RuntimeError::interrupted),
            "{sched:?}: {result:?}"
        );
    }
}

/// 右の子の閉包が捕まえた `MutArray` は、fork で子へ移り、子がその場で書き換える。join で親の局所に戻るので、親もその
/// 場で書き換えられる。印付けか移し戻しを外すと、検査モードが `WrongOwner` を返す。
#[test]
fn a_transferred_mut_array_is_written_by_both() {
    let (out, result) = run_par(
        "\
fn main() -> unit {
  let z.0: tobj = box 0
  let a.1: obj lin = extern Std.MutArray.make(2, z.0)
  let g.2: tobj lin = closure writer(a.1)
  let t.3: obj = par (,) #0(&one, g.2)
  unpack t.3 (,) #0(x.4: tobj, b.5: tobj lin)
  release t.3 (,) #0(_, b.5)
  let v.7: tobj = box 9
  let d.8: obj lin = extern Std.MutArray.write(b.5, 1, v.7)
  let r.12: obj = extern Std.MutArray.freeze(d.8)
  let e.9: tobj = extern Std.Array.get(r.12, 0)
  let f.10: tobj = extern Std.Array.get(r.12, 1)
  decref r.12
  let p.11: obj = con (,) #0(e.9, f.10)
  tail call show_pair(p.11)
}
fn writer(a.0: obj lin, p.1: unit) -> tobj lin {
  let v.2: tobj = box 5
  let b.3: obj lin = extern Std.MutArray.write(a.0, 0, v.2)
  return b.3
}
",
    );
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "5\n9\n");
}

/// 右の閉包が捕まえた `lin` でない組は親も持つので (rc 2)、fork はそれに共有の印を付けて、所有を移さない。右の子は
/// 組を読んで新しい組を返し、親は join の後で同じ組を読んで手放す。印付けを外すと、検査モードは子の読み込みを
/// `WrongOwner` で断る。共有にした組の rc を親と子が別々に減らしても、漏れも誤りもない。
#[test]
fn a_captured_value_the_parent_keeps_is_marked_and_shared() {
    let (out, result) = run_par(
        "\
fn main() -> unit {
  let a.0: obj = con (,) #0(#5, #6)
  dup a.0
  let g.1: tobj = closure swap(a.0)
  let t.2: obj = par (,) #0(&one, g.1)
  let (x.3: tobj, p.4: tobj) = unbox t.2 (,)
  dup p.4
  decref t.2
  let u.5: unit = call show_pair(p.4)
  tail call show_pair(a.0)
}
fn swap(a.0: obj, p.1: unit) -> tobj {
  let (x.2: tobj, y.3: tobj) = unbox a.0 (,)
  dup x.2
  dup y.3
  decref a.0
  let t.4: obj = con (,) #0(y.3, x.2)
  return t.4
}
",
    );
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "6\n5\n5\n6\n");
}

/// 右の子が返した `lin` の値は、移し戻しで親の局所の物体になるので、親が手放せる。
#[test]
fn a_lin_result_moves_back_to_the_parent() {
    let (out, result) = run_par(
        "\
fn main() -> unit {
  let z.0: tobj = box 3
  let a.1: obj lin = extern Std.MutArray.make(2, z.0)
  let g.2: tobj lin = closure keep(a.1)
  let t.3: obj = par (,) #0(&one, g.2)
  unpack t.3 (,) #0(x.4: tobj, b.5: tobj lin)
  release t.3 (,) #0(_, b.5)
  let r.10: obj = extern Std.MutArray.freeze(b.5)
  let e.7: tobj = extern Std.Array.get(r.10, 1)
  decref r.10
  let n.8: int = unbox e.7
  decref e.7
  let s.9: obj = extern \"Prelude.Show Int.show\"(n.8)
  tail call print(s.9)
}
fn keep(a.0: obj lin, p.1: unit) -> tobj lin {
  return a.0
}
fn print(s.0: obj) -> unit {
  let u.1: unit = extern Prelude.println(s.0)
  return u.1
}
",
    );
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "3\n");
}

/// 右の子が手放し忘れた物体は、join で右の子の ID を退かせるときに `TransferLeak` になる。`par` がなければ、同じ漏れは
/// 実行の終わりの `Leak` まで見つからない。verifier はこの IR を断るので、VM 自身の見張りとして検査モードだけで流す。
#[test]
fn a_child_that_leaks_fails_at_its_join() {
    let (_, result) = crate::common::run_unverified(&with_prelude(
        "\
fn main() -> unit {
  let t.0: obj = par (,) #0(&one, &leaky)
  decref t.0
  return ()
}
fn leaky(p.0: unit) -> tobj {
  let c.1: obj = con (,) #0(#1, #2)
  return ()
}
",
    ));
    assert_eq!(
        result,
        Err(RuntimeError::Fault {
            fault: VmFault::Extern(Fault::Heap(HeapError::TransferLeak { worker: 1, live: 1 })),
            function: "main".to_string(),
            at: None,
        })
    );
}

/// 長く走る2つの子の入れ子の `par` と handler。
const LONG: &str = "\
fn main() -> unit {
  let t.0: tobj = call with_outer(&body)
  tail call show_pair(t.0)
}
fn body(p.0: unit) -> tobj {
  let t.1: obj = par (,) #0(&left, &right)
  let (a.2: tobj, b.3: tobj) = unbox t.1 (,)
  dup a.2
  dup b.3
  decref t.1
  let u.4: unit = call show_pair(a.2)
  let u.5: unit = call show_pair(b.3)
  let t.6: obj = par (,) #0(&spin_one, &fail7)
  return t.6
}
fn left(p.0: unit) -> tobj {
  let t.1: obj = par (,) #0(&spin_one, &spin_two)
  return t.1
}
fn right(p.0: unit) -> tobj {
  let t.1: tobj = call with_inner(&inner)
  return t.1
}
fn inner(p.0: unit) -> tobj {
  let t.1: obj = par (,) #0(&spin_two, &spin_one)
  return t.1
}
";

/// seed の方針は長く走る子を横取りする。逐次の方針は横取りしない。どちらでも出力とスケジュールによらない `RunStats`
/// は同じである (`run_par_lir`)。
#[test]
fn seeds_preempt_long_children_and_sequential_never_preempts() {
    let (out, result) = run_par(LONG);
    assert_eq!(out, "1\n2\n2\n1\n7\n100\n");
    assert_eq!(result.map(|finished| finished.preemptions), Ok(0));
    let program = parse(&with_prelude(LONG));
    for seed in [1, 2026] {
        let (_, seeded) = run_in(&program, true, Sched::Seeded(seed), usize::MAX);
        let preemptions = seeded.map(|finished| finished.preemptions);
        assert!(
            preemptions.as_ref().is_ok_and(|&n| n > 0),
            "seed {seed}: {preemptions:?}"
        );
    }
}

/// 同じ seed は同じスケジュールを繰り返す。
#[test]
fn a_seed_reruns_the_same_schedule() {
    let program = parse(&with_prelude(LONG));
    let run = || run_in(&program, true, Sched::Seeded(7), usize::MAX);
    let (first_out, first) = run();
    let (second_out, second) = run();
    assert_eq!(first_out, second_out);
    assert_eq!(first, second);
}

/// 方針によらず `RunStats` が同じになるのは、切り替えが RC に触れず、fork と join の仕事が親の `evv` と子の値だけで
/// 決まるためである。同時に生きている物体の数だけは、子が交互に進む順で変わる。
#[test]
fn the_run_stats_do_not_depend_on_the_schedule() {
    let program = parse(&with_prelude(LONG));
    let (_, sequential) = run_in(&program, true, Sched::Sequential, usize::MAX);
    let sequential = sequential.expect("the sequential run").stats;
    assert!(sequential.allocations > 0 && sequential.unwound_frames > 0);
    for seed in [1, 2026] {
        let (_, seeded) = run_in(&program, true, Sched::Seeded(seed), usize::MAX);
        let seeded = seeded.expect("the seeded run").stats;
        assert_eq!(
            eml_test_support::schedule_independent(seeded),
            eml_test_support::schedule_independent(sequential),
            "seed {seed}"
        );
    }
}

/// 子の誤りの規則を、ソースから作った `fork$` と `reperform$` のコードで確かめる。上の手書きの LIR の版は VM の
/// 順序を、こちらは利用者の見る規則を確かめる (docs/implementation/testing.md の「重複させない」)。誤りは
/// 小さい深さの上限で起こす。既定の上限の `DepthLimit` は UI テストで起こすには大きすぎるためである。
mod source {
    use super::{Finished, RuntimeError, Sched, error_text, run_in, run_par_program};

    const DEEP: &str = "\
import Std.Par
import Fail (Fail(..))

deep_left : Int -> <Panic> Int
deep_left n = 1 + deep_left (n + 1)

deep_right : Int -> <Panic> Int
deep_right n = 1 + deep_right (n + 1)
";

    /// `run_par_program` は誤りで終わった run を速いモードで流さないので、ここで流して同じ文言を確かめる。深さの
    /// 上限は両方のモードが同じ文言で報告する。
    fn run(main: &str) -> (String, Result<Finished, RuntimeError>) {
        let text = format!(
            "{DEEP}\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  println \"before\"\n{main}"
        );
        let program = eml_test_support::lir(&text);
        let (out, result) = run_par_program(&program, 1000);
        let (fast_out, fast) = run_in(&program, false, Sched::Sequential, 1000);
        assert_eq!(fast_out, out, "the fast mode printed something else");
        assert_eq!(
            fast.map(|_| ()).map_err(|error| error.to_string()),
            result.as_ref().map(|_| ()).map_err(ToString::to_string),
            "the fast mode ended otherwise"
        );
        (out, result)
    }

    /// 左の子の中断は、右の子の誤りを救わない。
    #[test]
    fn a_left_abort_does_not_rescue_a_right_error() {
        let (out, result) = run(
            "  let r = Fail.to_result (fn () -> Par.par (fn () -> (Fail.fail \"l\" : Int)) (fn () -> deep_right 0))\n  println (show r)\n",
        );
        assert_eq!(out, "before\n");
        assert_eq!(
            error_text(result),
            "the call depth limit is exceeded in `deep_right`"
        );
    }

    /// 両方の子が誤りで止まれば、左の誤りを報告する。
    #[test]
    fn the_left_error_wins() {
        let (out, result) = run(
            "  let (a, b) = Par.par (fn () -> deep_left 0) (fn () -> deep_right 0)\n  println (show (a + b))\n",
        );
        assert_eq!(out, "before\n");
        assert_eq!(
            error_text(result),
            "the call depth limit is exceeded in `deep_left`"
        );
    }
}
