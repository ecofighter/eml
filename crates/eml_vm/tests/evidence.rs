//! evidence passing の基本操作と節の呼び出しを、EP の出力の形の手書きの LIR で確かめる
//! (docs/spec/core-ir.md の「evidence passing」と docs/spec/lir.md の「基本操作」)。

use eml_rt::{Fault, HeapError};
use eml_vm::{RuntimeError, VmFault};

use crate::common::run_lir;

const COUNTER: &str = "effect Counter { get(unit) -> tobj, put(tobj) -> unit }\n";

/// yield を始めるときの κ。EP の `id$` と同じ形で、`yield_to` の3つ目の引数に渡す。`()` を渡さない
/// のは、継続の欄の `()` が「取り出した後」の印を兼ねるためである (docs/spec/core-ir.md の「EP が作る補助の関数」)。
const ID: &str = "yields fn id(x.0: tobj) -> tobj {\n  return x.0\n}\n";

/// yield_to で始めた yield を、yield_status と yield_finish で受け取る。yield_status は、yield の前と後は 0、
/// 宛先の evidence では 1 (中断なら 2)、ほかの evidence では 3 である。`yield_abort` は戻らずに巻き戻すので、中断は
/// 呼んだ関数の中で始め、着地の印の付いた `apply` から状態を読む。
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
  let a.3: int = prim yield_status(e.1)
  let c.4: obj = const \"clause\"
  let u.6: unit = prim yield_to(e.1, c.4, &id)
  let y.7: int = prim yielding()
  let b.8: int = prim yield_status(e.1)
  let d.9: int = prim yield_status(e.2)
  decref e.2
  let k.10: tobj = prim yield_cont_take()
  let u.11: unit = prim yield_cont_set(k.10)
  let k.12: tobj = prim yield_cont_take()
  decref k.12
  let c.13: tobj = prim yield_finish()
  let f.14: int = prim yield_status(e.1)
  let z.15: int = prim yielding()
  let u.16: tobj = apply &abort(e.1) prompt e.1
  decref u.16
  let g.17: int = prim yield_status(e.1)
  decref e.1
  let c.19: tobj = prim yield_finish()
  decref c.19
  let p.20: unit = call show(a.3)
  let p.21: unit = call show(y.7)
  let p.22: unit = call show(b.8)
  let p.23: unit = call show(d.9)
  let p.24: unit = call show(f.14)
  let p.25: unit = call show(z.15)
  let p.26: unit = call show(g.17)
  tail call say(c.13)
}}
fn abort(e.0: tobj) -> tobj {{
  let u.1: unit = prim yield_abort(e.0, ())
  return u.1
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
  decref e.10
  let m.12: tobj = prim evv_mask[Counter](w.8)
  decref w.8
  let u.13: unit = prim evv_set(m.12)
  let e.14: obj = prim ev_lookup[Counter]()
  let s.15: tobj = prim ev_take_state(e.14)
  decref e.14
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

/// `Counter` を3つ積み、`Counter@1` で一番内側を残して次の1つを飛ばす。残った先頭は一番内側 (3)、そのリストの2つ目は
/// 飛ばした2ではなく一番外側 (1) である。2つ目は、先頭を飛ばす `evv_mask[Counter]` をもう一度かけて確かめる。
#[test]
fn evv_mask_keeps_the_innermost_evidence_and_skips_the_next() {
    let text = format!(
        "{COUNTER}\
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
  let s.7: tobj = box 3
  let o.8: obj = prim ev_new[Counter](s.7, w.0)
  let w.9: tobj = prim evv_push(w.6, o.8)
  let m.10: tobj = prim evv_mask[Counter@1](w.9)
  decref w.9
  dup m.10
  let u.11: unit = prim evv_set(m.10)
  let e.12: obj = prim ev_lookup[Counter]()
  let s.13: tobj = prim ev_take_state(e.12)
  decref e.12
  let m.14: tobj = prim evv_mask[Counter](m.10)
  decref m.10
  let u.15: unit = prim evv_set(m.14)
  let e.16: obj = prim ev_lookup[Counter]()
  let s.17: tobj = prim ev_take_state(e.16)
  decref e.16
  let u.18: unit = prim evv_set(w.0)
  let n.19: int = unbox s.13
  decref s.13
  let t.20: obj = extern \"Prelude.Show Int.show\"(n.19)
  let v.21: unit = extern Prelude.println(t.20)
  let n.22: int = unbox s.17
  decref s.17
  let t.23: obj = extern \"Prelude.Show Int.show\"(n.22)
  let v.24: unit = extern Prelude.println(t.23)
  return v.24
}}
"
    );
    let (output, result) = run_lir(&text);
    assert_eq!(result, Ok(()));
    assert_eq!(output, "3\n1\n");
}

/// handler の下で継続を再開するときの形。`ev_renew` はエントリの表を共有し、状態を差し替え、今の `evv` を外側の `evv`
/// にした evidence を作る (docs/spec/object-model.md の「evidence の3つの物体」)。元の evidence の外側の `evv` は空なので、
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
  decref e.2
  let o.8: tobj = prim ev_evv(r.7)
  let u.9: unit = prim evv_set(o.8)
  let f.10: obj = prim ev_lookup[Counter]()
  let a.11: tobj = prim ev_take_state(f.10)
  decref f.10
  let b.12: tobj = prim ev_take_state(r.7)
  decref r.7
  let u.13: unit = prim evv_set(())
  let p.14: unit = call show(a.11)
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

fn fault(fault: VmFault) -> RuntimeError {
    RuntimeError::Fault {
        fault,
        function: "main".to_string(),
        at: None,
    }
}

const LEFT_AT_END: &str = "the evidence list or a yield is left at the end";

fn heap_fault(error: HeapError) -> RuntimeError {
    fault(VmFault::Extern(Fault::Heap(error)))
}

fn internal_error(what: &'static str) -> RuntimeError {
    fault(VmFault::Internal(what))
}

/// 終わりに残る `evv` や yield は VM が `Ctx::check_finished` で見つけ、誤りの種類は分けない。
#[test]
fn a_program_that_ends_with_a_nonempty_evidence_list_is_an_error() {
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
    assert_eq!(error, internal_error(LEFT_AT_END));
    assert_eq!(
        error.to_string(),
        "internal error: the evidence list or a yield is left at the end in `main`"
    );
}

/// 終わりに残る `evv` や yield は VM が `Ctx::check_finished` で見つけ、誤りの種類は分けない。
#[test]
fn a_program_that_ends_during_a_yield_is_an_error() {
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
    assert_eq!(run_lir(&text).1, Err(internal_error(LEFT_AT_END)));
}

/// 2つ目の yield は、1つ目が終わる前には始まらない。`yield_abort` は検査モードで `YieldMisuse` を返す (`eml_rt` の `Ctx`)。
#[test]
fn a_yield_does_not_start_during_another_yield() {
    let text = format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: tobj = prim evv_get()
  let e.1: obj = prim ev_new[Counter]((), w.0)
  dup e.1
  let u.2: unit = prim yield_to(e.1, (), &id)
  let u.3: unit = prim yield_abort(e.1, ())
  return u.3
}}
{ID}"
    );
    assert_eq!(run_lir(&text).1, Err(heap_fault(HeapError::YieldMisuse)));
}

/// 今の `evv` に `Counter` の evidence がなければ、操作の evidence を探せない。`ev_lookup` は検査モードで `NoHandler` を返す。
#[test]
fn a_lookup_without_evidence_is_an_error() {
    let text = format!(
        "{COUNTER}\
fn main() -> unit {{
  let e.0: obj = prim ev_lookup[Counter]()
  decref e.0
  return ()
}}
"
    );
    assert_eq!(run_lir(&text).1, Err(heap_fault(HeapError::NoHandler)));
}

/// `mask` 付きの呼び出しの中の操作は、呼び出しより外側の同じエフェクトの handler を、`mask` に並ぶ数だけ飛ばす。
/// 呼び出しの中で設けた handler は飛ばさない (docs/spec/core-ir.md)。`ir/mask.lir` は、次のプログラムを Perceus
/// まで通した IR である。EP が `mask` を `evv_mask` と `mask$` の包みに置き換えている
/// (docs/spec/core-ir.md の「`mask` の包み」)。
///
/// ```text
/// effect Ask where
///   ask : Unit -> String
///
/// effect Yield where
///   yield : Unit -> Unit
///
/// asker : Unit -> <Ask, IO> Unit
/// asker () = println (ask ())
///
/// masked : (Unit -> <e> a) -> <Ask | e> a
/// masked f = f ()
///
/// rehandle : Unit -> <IO> Unit
/// rehandle () =
///   handle asker () with
///     | ctl ask () k -> k "new"
///
/// resumer : (Unit -> <Yield | e> Unit) -> (Unit -> <e> Unit) -> <e> Unit
/// resumer body after =
///   handle body () with
///     | ctl yield () k ->
///         handle k () with
///           | ctl ask () k2 -> k2 "clause"
///         after ()
///
/// yielder : Unit -> <Yield, Ask, IO> Unit
/// yielder () =
///   yield ()
///   asker ()
///
/// body : Unit -> <Ask, Ask, IO> Unit
/// body () =
///   masked asker
///   masked rehandle
///   resumer yielder asker
///   masked asker
///
/// main : Unit -> <IO, Panic> Unit
/// main () =
///   handle (handle body () with
///             | ctl ask () k -> k "inner") with
///     | ctl ask () k -> k "outer"
/// ```
///
/// `Ask` の handler を2つ積む。外側の handler は `outer`、内側の handler は `inner` を返す。内側の handler の下で、
/// 次の順に `ask` する。
///
/// 1. 末尾でない `mask` 付きの呼び出しで `ask` する (`outer`)
/// 2. `mask` 付きの呼び出しの中で3つめの handler を設けて `ask` する。その handler が `new` を返す
/// 3. `Yield` の節が、自分で設けた `Ask` の handler の下で `k` を `mask` 付きで再開し、本体が `ask` する。この `ask`
///    は節の handler を飛ばして `inner` に届く。`mask` がなければ `clause` になる
/// 4. 再開から戻った節で `ask` する。節は handle の外側で動くので `inner` になる
/// 5. 末尾の `mask` 付きの呼び出しで `ask` する (`outer`)
///
/// LIR のテキストにはコメントを書けないので、IR の説明はここに書く。
#[test]
fn a_mask_skips_outer_handlers_only() {
    let (stdout, result) = run_lir(include_str!("ir/mask.lir"));
    assert_eq!(result, Ok(()));
    insta::assert_snapshot!(stdout, @"
    outer
    new
    inner
    inner
    outer
    ");
}

/// `ir/multi_resume.lir` は、次のプログラムを Perceus まで通した IR である。`Choose` の節が `k` を2回再開し、
/// 内側の `Ask` の handle の `prompt$` は、再開のたびに `return` の節を通って戻る。LIR のテキストにはコメントを
/// 書けないので、IR の説明はここに書く。
///
/// ```text
/// effect Choose where
///   multi choose : Unit -> Int
///
/// effect Ask where
///   ask : Unit -> Int
///
/// main : Unit -> <IO, Panic> Unit
/// main () =
///   let r = handle (handle choose () with
///                     | ctl ask () k -> k 0
///                     | return x -> x + 100) with
///             | ctl choose () k -> k 1 + k 2
///   println (show r)
/// ```
#[test]
fn an_inner_handle_returns_through_each_resumption_of_an_outer_multi_operation() {
    let (stdout, result) = run_lir(include_str!("ir/multi_resume.lir"));
    assert_eq!(result, Ok(()));
    assert_eq!(stdout, "203\n");
}
