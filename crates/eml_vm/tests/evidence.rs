//! evidence passing の基本操作と節の呼び出しを、EP の出力の形の手書きの LIR で確かめる
//! (docs/spec/core-ir.md の「evidence passing」と docs/spec/lir.md の「基本操作」)。

use eml_rt::{Fault, HeapError};
use eml_sched::error::RunFault;
use eml_vm::RuntimeError;

use crate::common::{run_lir, run_unverified};

const COUNTER: &str = "effect Counter { get(unit) -> tobj, put(tobj) -> unit }\n";

/// `Counter`のエントリの関数。evidenceのエントリは`evidence`の命令に書く (docs/spec/lir.mdの「evidence」)。
/// `get`は状態を取り出して返すので、`clause Counter.get`でどのevidenceが見つかったかが分かる。
const COUNTER_ENTRIES: &str = "\
internal clause Counter.get fn get$entry(e.0: cev, u.1: unit) -> tobj {
  let s.2: tobj = prim ev_take_state(e.0)
  decref e.0
  return s.2
}
internal clause Counter.put fn put$entry(e.0: cev, x.1: tobj) -> unit {
  decref e.0
  decref x.1
  return ()
}
";

/// yield を始めるときの κ。EP の `id$` と同じ形で、`yield_to` の3つ目の引数に渡す。`()` を渡さない
/// のは、継続の欄の `()` が「取り出した後」の印を兼ねるためである (docs/spec/core-ir.md の「EP が作る補助の関数」)。
const ID: &str = "yields fn id(x.0: tobj) -> tobj {\n  return x.0\n}\n";

/// `yield_to`で始めたyieldを、`yield_status`と`yield_finish`で受け取る。`yield_status`は、yieldの前と後は0、宛先の
/// evidenceでは1 (中断なら2)、ほかのevidenceでは3である。`yield_to`と`yield_abort`は節が呼ばれるときに受けるevidenceを
/// 取るので、yieldは`Ask.ask`の節で、中断は`Ask.quit`の節で始める (docs/spec/lir.mdの「基本操作」)。`yield_abort`は戻ら
/// ずに巻き戻すので、節は呼んだ関数の中で起こし、着地の印の付いた`apply`の直後に呼ぶ小さな`prompt$`で状態を読む。
/// verifierがそこにその呼び出しを求めるためである (docs/spec/lir.mdの「着地の印」)。
#[test]
fn a_yield_reaches_the_status_of_its_target() {
    let text = format!(
        "{COUNTER}\
effect Ask {{ ask(unit) -> tobj, quit(unit) -> tobj }}
fn main() -> unit {{
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  let e.1: ev lin = evidence Ask((), w.0) [&ask$entry, &quit$entry]
  let e.2: ev lin = evidence Counter((), w.0) [&get$entry, &put$entry]
  let w.30: evv = prim evv_get()
  let w.31: evv = prim evv_enter(w.30, e.1)
  let u.32: unit = prim evv_set(w.31)
  let a.3: int = prim yield_status(e.1)
  let t.5: tobj = clause Ask.ask(())
  decref t.5
  let y.7: int = prim yielding()
  let b.8: int = prim yield_status(e.1)
  let d.9: int = prim yield_status(e.2)
  let u.94: unit = drop e.2
  let k.10: tobj = prim yield_cont_take()
  let u.11: unit = prim yield_cont_set(k.10)
  let k.12: tobj = prim yield_cont_take()
  decref k.12
  let c.13: tobj = prim yield_finish()
  let f.14: int = prim yield_status(e.1)
  let z.15: int = prim yielding()
  let u.16: tobj = apply &abort(()) prompt e.1
  let g.17: int = call prompt$(e.1, u.16)
  let u.33: unit = prim evv_set(w.0)
  let p.20: unit = call show(a.3)
  let p.21: unit = call show(y.7)
  let p.22: unit = call show(b.8)
  let p.23: unit = call show(d.9)
  let p.24: unit = call show(f.14)
  let p.25: unit = call show(z.15)
  let p.26: unit = call show(g.17)
  tail call say(c.13)
}}
fn abort(p.0: unit) -> tobj {{
  let r.1: tobj = clause Ask.quit(())
  return r.1
}}
fn prompt$(ev.0: ev lin, x.1: tobj) -> int {{
  decref x.1
  let s.2: int = prim yield_status(ev.0)
  let u.4: unit = drop ev.0
  let c.3: tobj = prim yield_finish()
  decref c.3
  return s.2
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
internal clause Ask.ask fn ask$entry(e.0: cev, u.1: unit) -> tobj {{
  let c.2: obj = const \"clause\"
  let u.3: unit = prim yield_to(e.0, c.2, &id)
  let d.4: obj poly = prim dummy()
  return d.4
}}
internal clause Ask.quit fn quit$entry(e.0: cev, u.1: unit) -> tobj {{
  let u.2: unit = prim yield_abort(e.0, ())
  let d.3: obj poly = prim dummy()
  return d.3
}}
{ID}{COUNTER_ENTRIES}"
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
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  dup w.0
  dup w.0
  let s.1: tobj = box 1
  let c.2: ev lin = evidence Counter(s.1, w.0) [&get$entry, &put$entry]
  let w.3: evv = prim evv_enter(w.0, c.2)
  let s.4: tobj = box 2
  let d.5: ev lin = evidence Counter(s.4, w.0) [&get$entry, &put$entry]
  let w.6: evv = prim evv_enter(w.3, d.5)
  let o.7: ev lin = evidence Other((), w.0) [&ask$entry]
  let w.8: evv = prim evv_enter(w.6, o.7)
  dup w.8
  let u.9: unit = prim evv_set(w.8)
  let s.11: tobj = clause Counter.get(())
  let m.12: evv = prim evv_mask[Counter](w.8)
  decref w.8
  let u.13: unit = prim evv_set(m.12)
  let s.15: tobj = clause Counter.get(())
  let u.16: unit = prim evv_set(w.0)
  let n.17: int = unbox s.11
  decref s.11
  let t.18: obj = extern \"Prelude.Show Int.show\"(n.17)
  let v.19: unit = extern Prelude.println(t.18)
  let n.20: int = unbox s.15
  decref s.15
  let t.21: obj = extern \"Prelude.Show Int.show\"(n.20)
  let v.22: unit = extern Prelude.println(t.21)
  let u.95: unit = drop c.2
  let u.96: unit = drop d.5
  let u.97: unit = drop o.7
  return v.22
}}
{COUNTER_ENTRIES}internal clause Other.ask fn ask$entry(e.0: cev, u.1: unit) -> tobj {{
  decref e.0
  return ()
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
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  dup w.0
  dup w.0
  let s.1: tobj = box 1
  let c.2: ev lin = evidence Counter(s.1, w.0) [&get$entry, &put$entry]
  let w.3: evv = prim evv_enter(w.0, c.2)
  let s.4: tobj = box 2
  let d.5: ev lin = evidence Counter(s.4, w.0) [&get$entry, &put$entry]
  let w.6: evv = prim evv_enter(w.3, d.5)
  let s.7: tobj = box 3
  let o.8: ev lin = evidence Counter(s.7, w.0) [&get$entry, &put$entry]
  let w.9: evv = prim evv_enter(w.6, o.8)
  let m.10: evv = prim evv_mask[Counter@1](w.9)
  decref w.9
  dup m.10
  let u.11: unit = prim evv_set(m.10)
  let s.13: tobj = clause Counter.get(())
  let m.14: evv = prim evv_mask[Counter](m.10)
  decref m.10
  let u.15: unit = prim evv_set(m.14)
  let s.17: tobj = clause Counter.get(())
  let u.18: unit = prim evv_set(w.0)
  let n.19: int = unbox s.13
  decref s.13
  let t.20: obj = extern \"Prelude.Show Int.show\"(n.19)
  let v.21: unit = extern Prelude.println(t.20)
  let n.22: int = unbox s.17
  decref s.17
  let t.23: obj = extern \"Prelude.Show Int.show\"(n.22)
  let v.24: unit = extern Prelude.println(t.23)
  let u.96: unit = drop c.2
  let u.97: unit = drop d.5
  let u.98: unit = drop o.8
  return v.24
}}
{COUNTER_ENTRIES}"
    );
    let (output, result) = run_lir(&text);
    assert_eq!(result, Ok(()));
    assert_eq!(output, "3\n1\n");
}

/// handlerの下で継続を再開するときの形。`ev_renew`は退いたevidenceのエントリの表を共有し、状態を差し替え、今の`evv`を外
/// 側の`evv`にしたevidenceを作る (docs/spec/object-model.mdの「evidence の3つの物体」)。退いたevidenceは外側の`evv`が空
/// のevidenceから作るので、`ev_take_evv`がその外側を返せば、節の呼び出しは今の`evv`が持つevidence`x.2`を見つけられない。
#[test]
fn a_renewed_evidence_has_its_own_state_and_the_current_evv_outside() {
    let text = format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  let s.1: tobj = box 1
  let x.2: ev lin = evidence Counter(s.1, w.0) [&get$entry, &put$entry]
  let w.3: evv = prim evv_get()
  let w.4: evv = prim evv_enter(w.3, x.2)
  let u.5: unit = prim evv_set(w.4)
  let e.20: ev lin = evidence Counter((), w.0) [&get$entry, &put$entry]
  let h.21: rev = prim ev_retire(e.20)
  let s.6: tobj = box 2
  let r.7: ev lin = prim ev_renew(h.21, s.6)
  decref h.21
  let o.8: evv = prim ev_take_evv(r.7)
  let u.9: unit = prim evv_set(o.8)
  let a.11: tobj = clause Counter.get(())
  let b.12: tobj = prim ev_last_state(r.7)
  let u.94: unit = drop r.7
  let u.13: unit = prim evv_set(w.0)
  let u.95: unit = drop x.2
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
{COUNTER_ENTRIES}"
    );
    assert_eq!(run_lir(&text), ("1\n2\n".to_string(), Ok(())));
}

/// `ev_retire`は、リストも持つevidenceを写す。写しはエントリの表を共有し、リストの中のevidenceはそのまま働く。新しくし
/// た写しは自分の状態を読み、その後の操作はリストの中のevidenceに届く。両方のモードで流し、検査モードは終わりに物体が残
/// らないことを確かめる (docs/spec/object-model.mdの「基本操作の契約」)。
#[test]
fn retiring_a_shared_evidence_copies_it() {
    let text = "\
effect Cell { get(unit) -> tobj, put(tobj) -> unit }
fn main() -> unit {
  let w.0: evv = prim evv_get()
  dup w.0
  let e.1: ev lin = evidence Cell((), w.0) [&get, &put]
  let w.2: evv = prim evv_get()
  let w.3: evv = prim evv_enter(w.2, e.1)
  let h.4: rev = prim ev_retire(e.1)
  let s.5: tobj = box 7
  let r.6: ev lin = prim ev_renew(h.4, s.5)
  decref h.4
  let o.7: evv = prim ev_take_evv(r.6)
  decref o.7
  let t.8: tobj = prim ev_last_state(r.6)
  let u.22: unit = drop r.6
  let u.23: unit = prim evv_set(w.3)
  let f.9: tobj = box 5
  let u.24: unit = clause Cell.put(f.9)
  let a.10: tobj = clause Cell.get(())
  let u.25: unit = prim evv_set(w.0)
  let p.11: unit = call show(t.8)
  tail call show(a.10)
}
fn show(s.0: tobj) -> unit {
  let n.1: int = unbox s.0
  decref s.0
  let t.2: obj = extern \"Prelude.Show Int.show\"(n.1)
  let u.3: unit = extern Prelude.println(t.2)
  return u.3
}
internal clause Cell.get fn get(e.0: cev, u.1: unit) -> tobj {
  let s.2: tobj = prim ev_take_state(e.0)
  dup s.2
  let v.3: unit = prim ev_set_state(e.0, s.2)
  decref e.0
  return s.2
}
internal clause Cell.put fn put(e.0: cev, n.1: tobj) -> unit {
  let u.2: unit = prim ev_set_state(e.0, n.1)
  decref e.0
  return u.2
}
";
    assert_eq!(run_lir(text), ("7\n5\n".to_string(), Ok(())));
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

fn fault(fault: RunFault) -> RuntimeError {
    RuntimeError::Fault {
        fault,
        function: "main".to_string(),
        at: None,
    }
}

const LEFT_AT_END: &str = "the evidence list or a yield is left at the end";

fn heap_fault(error: HeapError) -> RuntimeError {
    fault(RunFault::Extern(Fault::Heap(error)))
}

fn internal_error(what: &'static str) -> RuntimeError {
    fault(RunFault::Internal(what))
}

/// 終わりに残る `evv` や yield は VM が `Ctx::check_finished` で見つけ、誤りの種類は分けない。
#[test]
fn a_program_that_ends_with_a_nonempty_evidence_list_is_an_error() {
    let text = format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: evv = prim evv_get()
  dup w.0
  let e.1: ev lin = evidence Counter((), w.0) [&get$entry, &put$entry]
  let w.2: evv = prim evv_enter(w.0, e.1)
  let u.3: unit = prim evv_set(w.2)
  let u.92: unit = drop e.1
  return u.3
}}
{COUNTER_ENTRIES}"
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
  let w.0: evv = prim evv_get()
  let e.1: ev lin = evidence Counter((), w.0) [&get$entry, &put$entry]
  let u.2: unit = prim yield_to(e.1, (), &id)
  return u.2
}}
{ID}{COUNTER_ENTRIES}"
    );
    assert_eq!(run_unverified(&text).1, Err(internal_error(LEFT_AT_END)));
}

/// 2つ目の yield は、1つ目が終わる前には始まらない。`yield_abort` は検査モードで `YieldMisuse` を返す (`eml_rt` の `Ctx`)。
#[test]
fn a_yield_does_not_start_during_another_yield() {
    let text = format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: evv = prim evv_get()
  let e.1: ev lin = evidence Counter((), w.0) [&get$entry, &put$entry]
  dup e.1
  let u.2: unit = prim yield_to(e.1, (), &id)
  let u.3: unit = prim yield_abort(e.1, ())
  return u.3
}}
{ID}{COUNTER_ENTRIES}"
    );
    assert_eq!(
        run_unverified(&text).1,
        Err(heap_fault(HeapError::YieldMisuse))
    );
}

/// 今の`evv`に`Counter`のevidenceがなければ、節の呼び出しは操作のevidenceを探せない。`ev_lookup`は検査モードで
/// `NoHandler`を返す。
#[test]
fn a_lookup_without_evidence_is_an_error() {
    let text = format!(
        "{COUNTER}\
fn main() -> unit {{
  let s.0: tobj = clause Counter.get(())
  decref s.0
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
