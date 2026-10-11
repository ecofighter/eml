//! evidence passing の基本操作と節の呼び出しを、EP の出力の形の手書きの LIR で確かめる
//! (docs/spec/core-ir.md の「evidence passing」と docs/spec/lir.md の「基本操作」)。

use eml_interp::{Fault, RunConfig, RuntimeError};
use eml_runtime::OutputSink;

use crate::common::{parse, run_lir};

const COUNTER: &str = "effect Counter { get(unit) -> tobj, put(tobj) -> unit }\n";

/// yield を始めるときの κ。EP の `id$` と同じ形で、`yield_to` と `yield_abort` の3つ目の引数に渡す。`()` を渡さない
/// のは、継続の欄の `()` が「取り出した後」の印を兼ねるためである (docs/spec/core-ir.md の「EP が作る補助の関数」)。
const ID: &str = "yields fn id(x.0: tobj) -> tobj {\n  return x.0\n}\n";

/// すぐに再開する節を evidence から引いてその場で呼ぶ。状態は evidence の欄に残る。
#[test]
fn an_immediate_clause_reads_and_writes_the_state_of_its_evidence() {
    let text = format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: tobj = prim evv_get()
  dup w.0
  dup w.0
  let s.1: tobj = box 3
  let e.2: obj = prim ev_new[Counter](s.1, w.0)
  dup e.2
  dup e.2
  let u.3: unit = prim ev_set_clause[Counter.get](e.2, &get)
  let u.4: unit = prim ev_set_clause[Counter.put](e.2, &put)
  let w.5: tobj = prim evv_push(w.0, e.2)
  let u.6: unit = prim evv_set(w.5)
  let e.7: obj = prim ev_lookup[Counter]()
  dup e.7
  let c.8: tobj = prim ev_clause[Counter.put](e.7)
  let f.9: tobj = box 5
  let u.10: unit = clause Counter.put c.8(e.7, f.9) save [w.0]
  let e.11: obj = prim ev_lookup[Counter]()
  dup e.11
  let c.12: tobj = prim ev_clause[Counter.get](e.11)
  let r.13: tobj = clause Counter.get c.12(e.11, ()) save [w.0]
  let u.14: unit = prim evv_set(w.0)
  let n.15: int = unbox r.13
  decref r.13
  let t.16: obj = extern \"Prelude.Show Int.show\"(n.15)
  let v.17: unit = extern Prelude.println(t.16)
  return v.17
}}
internal yields clause Counter.get fn get(e.0: obj, u.1: unit) -> tobj {{
  dup e.0
  let s.2: tobj = prim ev_take_state(e.0)
  dup s.2
  let v.3: unit = prim ev_set_state(e.0, s.2)
  return s.2
}}
internal yields clause Counter.put fn put(e.0: obj, n.1: tobj) -> unit {{
  let u.2: unit = prim ev_set_state(e.0, n.1)
  return u.2
}}
"
    );
    let (output, result) = run_lir(&text);
    assert_eq!(result, Ok(()));
    assert_eq!(output, "5\n");
}

/// yield_to で始めた yield を、yield_status と yield_finish で受け取る。yield_status は、yield の前と後は 0、
/// 宛先の evidence では 1 (中断なら 2)、ほかの evidence では 3 である。
#[test]
fn a_yield_reaches_the_status_of_its_target() {
    let text = format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: tobj = prim evv_get()
  dup w.0
  let e.1: obj = prim ev_new[Counter]((), w.0)
  let e.2: obj = prim ev_new[Counter]((), w.0)
  dup e.1
  dup e.1
  dup e.1
  dup e.1
  dup e.1
  let a.3: int = prim yield_status(e.1)
  let c.4: obj = const \"clause\"
  let u.6: unit = prim yield_to(e.1, c.4, &id)
  let y.7: int = prim yielding()
  let b.8: int = prim yield_status(e.1)
  let d.9: int = prim yield_status(e.2)
  let k.10: tobj = prim yield_cont_take()
  let u.11: unit = prim yield_cont_set(k.10)
  let k.12: tobj = prim yield_cont_take()
  decref k.12
  let c.13: tobj = prim yield_finish()
  let f.14: int = prim yield_status(e.1)
  let z.15: int = prim yielding()
  let u.16: unit = prim yield_abort(e.1, (), &id)
  let g.17: int = prim yield_status(e.1)
  let k.18: tobj = prim yield_cont_take()
  decref k.18
  let c.19: tobj = prim yield_finish()
  decref c.19
  let p.20: unit = call show(a.3) save [y.7, b.8, d.9, c.13, f.14, z.15, g.17]
  let p.21: unit = call show(y.7) save [b.8, d.9, c.13, f.14, z.15, g.17]
  let p.22: unit = call show(b.8) save [d.9, c.13, f.14, z.15, g.17]
  let p.23: unit = call show(d.9) save [c.13, f.14, z.15, g.17]
  let p.24: unit = call show(f.14) save [c.13, z.15, g.17]
  let p.25: unit = call show(z.15) save [c.13, g.17]
  let p.26: unit = call show(g.17) save [c.13]
  tail call say(c.13)
}}
fn say(s.0: obj) -> unit {{
  let u.1: unit = extern Prelude.println(s.0)
  return u.1
}}
fn show(n.0: int) -> unit {{
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}}
{ID}"
    );
    let (output, result) = run_lir(&text);
    assert_eq!(result, Ok(()));
    assert_eq!(output, "0\n1\n1\n3\n0\n0\n2\nclause\n");
}

/// `Other` の evidence の後ろに `Counter` を2つ積む。mask は内側の `Counter` を飛ばし、その前の `Other` の節点を写し、
/// 外側の `Counter` からは元のリストを共有する。
#[test]
fn evv_mask_skips_the_innermost_evidence_of_an_effect() {
    let text = format!(
        "{COUNTER}\
effect Other {{ ask(unit) -> tobj }}
fn main() -> unit {{
  let w.0: tobj = prim evv_get()
  dup w.0
  dup w.0
  dup w.0
  dup w.0
  let s.1: tobj = box 1
  let c.2: obj = prim ev_new[Counter](s.1, w.0)
  let w.3: tobj = prim evv_push(w.0, c.2)
  let s.4: tobj = box 2
  let d.5: obj = prim ev_new[Counter](s.4, w.0)
  let w.6: tobj = prim evv_push(w.3, d.5)
  let o.7: obj = prim ev_new[Other]((), w.0)
  let w.8: tobj = prim evv_push(w.6, o.7)
  dup w.8
  let u.9: unit = prim evv_set(w.8)
  let e.10: obj = prim ev_lookup[Counter]()
  let s.11: tobj = prim ev_take_state(e.10)
  let m.12: tobj = prim evv_mask[Counter](w.8)
  let u.13: unit = prim evv_set(m.12)
  let e.14: obj = prim ev_lookup[Counter]()
  let s.15: tobj = prim ev_take_state(e.14)
  let u.16: unit = prim evv_set(w.0)
  let n.17: int = unbox s.11
  decref s.11
  let t.18: obj = extern \"Prelude.Show Int.show\"(n.17)
  let v.19: unit = extern Prelude.println(t.18)
  let n.20: int = unbox s.15
  decref s.15
  let t.21: obj = extern \"Prelude.Show Int.show\"(n.20)
  let v.22: unit = extern Prelude.println(t.21)
  return v.22
}}
"
    );
    let (output, result) = run_lir(&text);
    assert_eq!(result, Ok(()));
    assert_eq!(output, "2\n1\n");
}

/// handler の下で継続を再開するときの形。`ev_renew` は入口の表を共有し、状態を差し替え、今の `evv` を外側の `evv`
/// にした evidence を作る (docs/spec/runtime.md の「evidence と `evv` の物体」)。元の evidence の外側の `evv` は空なので、
/// `ev_evv` が元の外側を返せば `ev_lookup` は evidence を見つけられない。
#[test]
fn a_renewed_evidence_has_its_own_state_and_the_current_evv_outside() {
    let text = format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: tobj = prim evv_get()
  let s.1: tobj = box 1
  let e.2: obj = prim ev_new[Counter](s.1, w.0)
  dup e.2
  let w.3: tobj = prim evv_get()
  let w.4: tobj = prim evv_push(w.3, e.2)
  let u.5: unit = prim evv_set(w.4)
  let s.6: tobj = box 2
  let r.7: obj = prim ev_renew(e.2, s.6)
  dup r.7
  let o.8: tobj = prim ev_evv(r.7)
  let u.9: unit = prim evv_set(o.8)
  let f.10: obj = prim ev_lookup[Counter]()
  let a.11: tobj = prim ev_take_state(f.10)
  let b.12: tobj = prim ev_take_state(r.7)
  let u.13: unit = prim evv_set(())
  let p.14: unit = call show(a.11) save [b.12]
  tail call show(b.12)
}}
fn show(s.0: tobj) -> unit {{
  let n.1: int = unbox s.0
  decref s.0
  let t.2: obj = extern \"Prelude.Show Int.show\"(n.1)
  let u.3: unit = extern Prelude.println(t.2)
  return u.3
}}
"
    );
    assert_eq!(run_lir(&text), ("1\n2\n".to_string(), Ok(())));
}

/// `make` が返すダミーを、`unbox` し、`box` し直し、`dup` と `decref` に通す。
const DUMMY: &str = "\
fn main() -> unit {
  let x.0: tobj = call make()
  let n.1: int = unbox x.0
  dup x.0
  decref x.0
  decref x.0
  let b.2: tobj = box n.1
  decref b.2
  return ()
}
fn make() -> tobj {
  let d.0: obj = prim dummy()
  return d.0
}
";

#[test]
fn a_dummy_passes_through_box_unbox_dup_and_decref() {
    assert_eq!(run_lir(DUMMY), (String::new(), Ok(())));
}

/// ダミーはヒープに置かないので、ダミーの代わりに `box 0` を返すプログラムと、同時に生きていた物体の数が同じである。
/// `box 0` の整数もヒープに置かない。
#[test]
fn a_dummy_is_not_a_heap_object() {
    let peak = |text: &str| {
        let program = parse(text);
        eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
        let (sink, _captured) = OutputSink::capture();
        let config = RunConfig::default().with_debug_heap(true);
        eml_interp::run(&program, &config, &sink)
            .unwrap()
            .peak_objects
    };
    let boxed = DUMMY.replace("let d.0: obj = prim dummy()", "let d.0: tobj = box 0");
    assert_ne!(boxed, DUMMY);
    assert_eq!(peak(DUMMY), peak(&boxed));
}

fn internal_error(what: &'static str) -> RuntimeError {
    RuntimeError::Fault {
        fault: Fault::Internal(what),
        function: "main".to_string(),
        at: None,
    }
}

#[test]
fn a_program_that_ends_with_a_nonempty_evidence_list_is_an_internal_error() {
    let text = format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: tobj = prim evv_get()
  dup w.0
  let e.1: obj = prim ev_new[Counter]((), w.0)
  let w.2: tobj = prim evv_push(w.0, e.1)
  let u.3: unit = prim evv_set(w.2)
  return u.3
}}
"
    );
    let (_, result) = run_lir(&text);
    let error = result.unwrap_err();
    assert_eq!(
        error,
        internal_error("the evidence list is not empty at the end")
    );
    assert_eq!(
        error.to_string(),
        "internal error: the evidence list is not empty at the end in `main`"
    );
}

#[test]
fn a_program_that_ends_during_a_yield_is_an_internal_error() {
    let text = format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: tobj = prim evv_get()
  let e.1: obj = prim ev_new[Counter]((), w.0)
  let u.2: unit = prim yield_to(e.1, (), &id)
  return u.2
}}
{ID}"
    );
    assert_eq!(
        run_lir(&text).1,
        Err(internal_error("a yield is left at the end"))
    );
}

/// 2つ目の yield は、1つ目が終わる前には始まらない。
#[test]
fn a_yield_does_not_start_during_another_yield() {
    let text = format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: tobj = prim evv_get()
  let e.1: obj = prim ev_new[Counter]((), w.0)
  dup e.1
  let u.2: unit = prim yield_to(e.1, (), &id)
  let u.3: unit = prim yield_abort(e.1, (), &id)
  return u.3
}}
{ID}"
    );
    assert_eq!(
        run_lir(&text).1,
        Err(internal_error("a yield starts during another yield"))
    );
}

/// 今の `evv` に `Counter` の evidence がなければ、操作の evidence を探せない。
#[test]
fn a_lookup_without_evidence_is_an_internal_error() {
    let text = format!(
        "{COUNTER}\
fn main() -> unit {{
  let e.0: obj = prim ev_lookup[Counter]()
  decref e.0
  return ()
}}
"
    );
    assert_eq!(
        run_lir(&text).1,
        Err(internal_error("an operation without evidence"))
    );
}
