//! エフェクトの基本操作のテスト (docs/spec/native.md の「エフェクト」)。
use eml_lir::{Prim, Repr, Step};
use eml_native::Arg;
use eml_nrt::image::{DATUM_EFFECT, DATUM_FN, DATUM_MASK, DatumRow, MaskRow};
use eml_nrt::{RunFault, RuntimeError};
use eml_rt::{Ctx, Fast, Fault, HeapError, Word};

use crate::common::native::{
    Visit, Visits, agree, agree_source, call_visited, call_visited_at, clif, machine, native,
    program, visit, visit_each,
};

/// 即値の数。`evv_mask`は項目の列の長さ、`ev_new`は操作の数、`ev_set_clause`、`root_get`、`root_set`は番号である。
fn immediates(prim: &Prim) -> usize {
    match prim.name() {
        "evv_mask" | "ev_new" => 2,
        "ev_set_clause" | "root_get" | "root_set" => 1,
        _ => 0,
    }
}

/// 呼ぶ基本操作のエントリは、記号が`eml_rt_`に基本操作の名前を続けたもので、引数は即値と行の引数を並べたものである。
/// 表を持たず、基本操作の表から導く。インラインの2つと、`empty_mut_array`、`child_abort`にはエントリがない。命令が
/// 走らせる`Ctx`の操作のエントリも、記号が`eml_rt_`にstepの名前を続けたもので、stepのオペランドを受け、stepが返す
/// ときに語を返す。
#[test]
fn every_prim_entry_is_named_after_its_prim() {
    const NO_ENTRY: [&str; 4] = ["yielding", "yield_status", "empty_mut_array", "child_abort"];
    for prim in Prim::KINDS {
        let symbol = format!("eml_rt_{}", prim.name());
        let entry = eml_nrt::entries().find(|e| e.symbol == symbol);
        if NO_ENTRY.contains(&prim.name()) {
            assert!(entry.is_none(), "{symbol}");
            continue;
        }
        let entry = entry.unwrap_or_else(|| panic!("no entry {symbol}"));
        let row = prim.row();
        assert_eq!(
            entry.params.len(),
            immediates(&prim) + row.params.len(),
            "{symbol}"
        );
        assert_eq!(entry.result.is_none(), row.ret == Repr::Unit, "{symbol}");
    }
    for step in Step::ALL {
        let symbol = format!("eml_rt_{}", step.name());
        let entry = eml_nrt::entries()
            .find(|e| e.symbol == symbol)
            .unwrap_or_else(|| panic!("no entry {symbol}"));
        assert_eq!(entry.params.len(), step.operands(), "{symbol}");
        assert_eq!(entry.result.is_some(), step.returns(), "{symbol}");
    }
}

const COUNTER: &str = "effect Counter { get(unit) -> tobj, put(tobj) -> unit }\n";

/// `Counter`のエントリの関数。evidenceのエントリは、どれも`ev_new`の直後に入れる (docs/spec/lir.mdの「verifier」)。
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

/// `Other`のエントリの関数。`Counter`の`get`と同じく状態を取り出して返す。
const OTHER_ENTRY: &str = "\
internal clause Other.ask fn ask$entry(e.0: cev, u.1: unit) -> tobj {
  let s.2: tobj = prim ev_take_state(e.0)
  decref e.0
  return s.2
}
";

/// 節がyieldを始める`Ask`。`ask`は`clause`を渡して自分のevidenceへyieldし、`quit`はそこへ中断する。
const ASK: &str = "effect Ask { ask(unit) -> tobj, quit(unit) -> tobj }\n";
const ASK_ENTRIES: &str = "\
internal clause Ask.ask fn ask$entry(e.0: cev, u.1: unit) -> tobj {
  let c.2: obj = const \"clause\"
  let u.3: unit = prim yield_to(e.0, c.2, &id)
  let d.4: obj poly = prim dummy()
  return d.4
}
internal clause Ask.quit fn quit$entry(e.0: cev, u.1: unit) -> tobj {
  let u.2: unit = prim yield_abort(e.0, ())
  let d.3: obj poly = prim dummy()
  return d.3
}
";

/// yieldを始めるときのκ。EPの`id$`と同じ形である。
const ID: &str = "yields fn id(x.0: tobj) -> tobj {\n  return x.0\n}\n";

/// `int`を1行に表示する関数。
const SHOW: &str = "\
fn show(n.0: int) -> unit {
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
";

/// 箱にした`int`を表示して手放す関数。
const SHOW_BOXED: &str = "\
fn show_boxed(s.0: tobj) -> unit {
  let n.1: int = unbox s.0
  decref s.0
  tail call show(n.1)
}
";

/// `yield_status`はVMと同じに読める。yieldの前と後は0、宛先のevidenceでは1 (中断なら2)、ほかのevidenceでは3である。
/// `yield_to`と`yield_abort`は節が呼ばれるときに受けるevidenceを取るので、`Ask.ask`の節がyieldを、`Ask.quit`の節が中断
/// を始める。中断の状態は、中断が着地する、着地の印の付いた`apply`の直後に呼ぶ小さな`prompt$`で読む (docs/spec/lir.mdの
/// 「着地の印」)。
#[test]
fn yield_status_reads_as_the_runtime_does() {
    agree(&format!(
        "{COUNTER}{ASK}\
fn main() -> unit {{
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  let e.1: ev lin = prim ev_new[Ask]((), w.0)
  let u.90: unit = prim ev_set_clause[Ask.ask](e.1, &ask$entry)
  let u.91: unit = prim ev_set_clause[Ask.quit](e.1, &quit$entry)
  let e.2: ev lin = prim ev_new[Counter]((), w.0)
  let u.92: unit = prim ev_set_clause[Counter.get](e.2, &get$entry)
  let u.93: unit = prim ev_set_clause[Counter.put](e.2, &put$entry)
  let w.30: evv = prim evv_get()
  let w.31: evv = prim evv_enter(w.30, e.1)
  let u.32: unit = prim evv_set(w.31)
  let a.3: int = prim yield_status(e.1)
  let t.5: tobj = clause Ask.ask(())
  decref t.5
  let b.6: int = prim yield_status(e.1)
  let d.7: int = prim yield_status(e.2)
  let u.94: unit = drop e.2
  let k.8: tobj = prim yield_cont_take()
  decref k.8
  let c.9: tobj = prim yield_finish()
  decref c.9
  let f.10: int = prim yield_status(e.1)
  let u.16: tobj = apply &abort(()) prompt e.1
  let g.17: int = call prompt$(e.1, u.16)
  let u.33: unit = prim evv_set(w.0)
  let p.11: unit = call show(a.3)
  let p.12: unit = call show(b.6)
  let p.13: unit = call show(d.7)
  let p.14: unit = call show(f.10)
  tail call show(g.17)
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
{SHOW}{ID}{COUNTER_ENTRIES}{ASK_ENTRIES}"
    ));
}

/// `yielding`はyieldの間だけ1である。κを取り出して置き直しても変わらない。
#[test]
fn yielding_is_one_while_a_yield_is_in_flight() {
    agree(&format!(
        "{ASK}\
fn main() -> unit {{
  let a.0: int = prim yielding()
  let w.1: evv = prim evv_get()
  dup w.1
  let e.2: ev lin = prim ev_new[Ask]((), w.1)
  let u.90: unit = prim ev_set_clause[Ask.ask](e.2, &ask$entry)
  let u.91: unit = prim ev_set_clause[Ask.quit](e.2, &quit$entry)
  let w.20: evv = prim evv_get()
  let w.21: evv = prim evv_enter(w.20, e.2)
  let u.22: unit = prim evv_set(w.21)
  let t.4: tobj = clause Ask.ask(())
  decref t.4
  let b.5: int = prim yielding()
  let k.6: tobj = prim yield_cont_take()
  let u.7: unit = prim yield_cont_set(k.6)
  let d.8: int = prim yielding()
  let k.9: tobj = prim yield_cont_take()
  decref k.9
  let c.10: tobj = prim yield_finish()
  decref c.10
  let f.11: int = prim yielding()
  let u.23: unit = prim evv_set(w.1)
  let p.12: unit = call show(a.0)
  let p.13: unit = call show(b.5)
  let p.14: unit = call show(d.8)
  let u.92: unit = drop e.2
  tail call show(f.11)
}}
{SHOW}{ID}{ASK_ENTRIES}"
    ));
}

/// handlerを設ける基本操作と、節の呼び出しと、evidenceを読み書きする基本操作を、VMと同じに走らせる。
#[test]
fn a_handler_is_installed_and_found() {
    agree(&format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  let s.1: tobj = box 5
  let e.2: ev lin = prim ev_new[Counter](s.1, w.0)
  let u.4: unit = prim ev_set_clause[Counter.get](e.2, &get)
  let u.6: unit = prim ev_set_clause[Counter.put](e.2, &put)
  let w.7: evv = prim evv_enter(w.0, e.2)
  let u.8: unit = prim evv_set(w.7)
  let p.5: tobj = box 20
  let q.21: unit = clause Counter.put(p.5)
  let q.26: tobj = clause Counter.get(())
  let o.25: evv = prim ev_take_evv(e.2)
  decref o.25
  let v.17: tobj = prim ev_last_state(e.2)
  let h.32: rev = prim ev_retire(e.2)
  let s.15: tobj = box 7
  let r.16: ev lin = prim ev_renew(h.32, s.15)
  decref h.32
  let x.18: tobj = prim ev_last_state(r.16)
  let u.31: unit = drop r.16
  let u.20: unit = prim evv_set(w.0)
  let q.29: unit = call show_boxed(q.26)
  let q.23: unit = call show_boxed(v.17)
  let q.24: unit = call show_boxed(x.18)
  let d.27: tobj = prim dummy()
  let n.28: int = unbox d.27
  decref d.27
  tail call show(n.28)
}}
internal clause Counter.get fn get(e.0: cev, u.1: unit) -> tobj {{
  let t.2: tobj = prim ev_take_state(e.0)
  let s.3: tobj = box 6
  let u.4: unit = prim ev_set_state(e.0, s.3)
  decref e.0
  return t.2
}}
internal clause Counter.put fn put(e.0: cev, v.1: tobj) -> unit {{
  let o.2: evv = prim ev_evv(e.0)
  decref o.2
  decref e.0
  tail call show_boxed(v.1)
}}
{SHOW_BOXED}{SHOW}"
    ));
}

/// `Counter`を2つと`Other`を1つ積み、`evv_mask[Counter]`が内側の`Counter`を飛ばす。項目が2つのマスクも同じに流す。
#[test]
fn a_mask_skips_the_named_evidence() {
    agree(&format!(
        "{COUNTER}\
effect Other {{ ask(unit) -> tobj }}
fn main() -> unit {{
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  dup w.0
  dup w.0
  let s.1: tobj = box 1
  let c.2: ev lin = prim ev_new[Counter](s.1, w.0)
  let u.90: unit = prim ev_set_clause[Counter.get](c.2, &get$entry)
  let u.91: unit = prim ev_set_clause[Counter.put](c.2, &put$entry)
  let w.3: evv = prim evv_enter(w.0, c.2)
  let s.4: tobj = box 2
  let d.5: ev lin = prim ev_new[Counter](s.4, w.0)
  let u.92: unit = prim ev_set_clause[Counter.get](d.5, &get$entry)
  let u.93: unit = prim ev_set_clause[Counter.put](d.5, &put$entry)
  let w.6: evv = prim evv_enter(w.3, d.5)
  let o.7: ev lin = prim ev_new[Other]((), w.0)
  let u.94: unit = prim ev_set_clause[Other.ask](o.7, &ask$entry)
  let w.8: evv = prim evv_enter(w.6, o.7)
  dup w.8
  let u.9: unit = prim evv_set(w.8)
  let s.11: tobj = clause Counter.get(())
  let m.12: evv = prim evv_mask[Counter](w.8)
  decref w.8
  let u.13: unit = prim evv_set(m.12)
  let s.15: tobj = clause Counter.get(())
  let u.16: unit = prim evv_set(w.0)
  let p.17: unit = call show_boxed(s.11)
  let u.95: unit = drop c.2
  let u.96: unit = drop d.5
  let u.97: unit = drop o.7
  tail call show_boxed(s.15)
}}
{SHOW_BOXED}{SHOW}{COUNTER_ENTRIES}{OTHER_ENTRY}"
    ));
    // 項目が2つのマスクは、`m:`のデータの物から項目を2つ読む。内側の`Counter`と内側の`Other`を飛ばすので、どちらの
    // エフェクトでも外側のevidenceが見つかる
    agree(&format!(
        "{COUNTER}\
effect Other {{ ask(unit) -> tobj }}
fn main() -> unit {{
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  dup w.0
  dup w.0
  dup w.0
  let s.1: tobj = box 1
  let c.2: ev lin = prim ev_new[Counter](s.1, w.0)
  let u.95: unit = prim ev_set_clause[Counter.get](c.2, &get$entry)
  let u.96: unit = prim ev_set_clause[Counter.put](c.2, &put$entry)
  let w.3: evv = prim evv_enter(w.0, c.2)
  let s.4: tobj = box 2
  let d.5: ev lin = prim ev_new[Counter](s.4, w.0)
  let u.97: unit = prim ev_set_clause[Counter.get](d.5, &get$entry)
  let u.98: unit = prim ev_set_clause[Counter.put](d.5, &put$entry)
  let w.6: evv = prim evv_enter(w.3, d.5)
  let s.7: tobj = box 3
  let o.8: ev lin = prim ev_new[Other](s.7, w.0)
  let u.99: unit = prim ev_set_clause[Other.ask](o.8, &ask$entry)
  let w.9: evv = prim evv_enter(w.6, o.8)
  let s.10: tobj = box 4
  let q.11: ev lin = prim ev_new[Other](s.10, w.0)
  let u.100: unit = prim ev_set_clause[Other.ask](q.11, &ask$entry)
  let w.12: evv = prim evv_enter(w.9, q.11)
  let m.13: evv = prim evv_mask[Counter, Other](w.12)
  decref w.12
  let u.14: unit = prim evv_set(m.13)
  let s.16: tobj = clause Counter.get(())
  let s.18: tobj = clause Other.ask(())
  let u.19: unit = prim evv_set(w.0)
  let p.20: unit = call show_boxed(s.16)
  let u.101: unit = drop c.2
  let u.102: unit = drop d.5
  let u.103: unit = drop o.8
  let u.104: unit = drop q.11
  tail call show_boxed(s.18)
}}
{SHOW_BOXED}{SHOW}{COUNTER_ENTRIES}{OTHER_ENTRY}"
    ));
}

/// `text`を読み込んだマシンの`Image`の、種類が`kind`のデータの行。
fn datum_rows(text: &str, kind: u32) -> Vec<DatumRow> {
    let program = program(text);
    let mut machine = machine();
    machine
        .load(&program, &eml_test_support::link(&program))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let image = machine.image().expect("a program is loaded");
    // SAFETY: マシンが生きている間、`Image`の表はマシンのもので変わらない
    let rows = unsafe { image.data.as_slice() }
        .iter()
        .filter(|row| row.kind == kind)
        .copied()
        .collect();
    machine.close();
    rows
}

/// マスクのデータの行は、項目をLIRの順に、マシンのエフェクトの番号で持つ。
#[test]
fn a_mask_datum_holds_its_items_in_lir_order() {
    let text = "\
effect A { a(unit) -> tobj }
effect B { b(unit) -> tobj }
fn main() -> unit {
  return ()
}
fn f(w.0: evv) -> unit {
  let m.1: evv = prim evv_mask[A, B@1*2](w.0)
  decref w.0
  let u.2: unit = prim evv_set(m.1)
  return u.2
}
";
    let program = program(text);
    let mut machine = machine();
    machine
        .load(&program, &eml_test_support::link(&program))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let image = machine.image().expect("a program is loaded");
    // SAFETY: マシンが生きている間、`Image`の表はマシンのもので変わらない
    let items: Vec<Vec<MaskRow>> = unsafe {
        image
            .data
            .as_slice()
            .iter()
            .filter(|row| row.kind == DATUM_MASK)
            .map(|row| row.items.as_slice().to_vec())
            .collect()
    };
    machine.close();
    assert_eq!(
        items,
        [vec![
            MaskRow {
                effect: 0,
                keep: 0,
                skip: 1
            },
            MaskRow {
                effect: 1,
                keep: 1,
                skip: 2
            },
        ]]
    );
}

/// 2つのエフェクトを両方の順に宣言したプログラム。`f`は`A`の節の呼び出しで、`A`のevidenceの状態を読む。
fn two_effects(first: &str, second: &str) -> String {
    format!(
        "effect {first} {{ {}(unit) -> tobj }}
effect {second} {{ {}(unit) -> tobj }}
fn f() -> int {{
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  dup w.0
  let s.1: tobj = box 1
  let a.2: ev lin = prim ev_new[A](s.1, w.0)
  let u.90: unit = prim ev_set_clause[A.a](a.2, &a$entry)
  let w.3: evv = prim evv_enter(w.0, a.2)
  let s.4: tobj = box 2
  let b.5: ev lin = prim ev_new[B](s.4, w.0)
  let u.91: unit = prim ev_set_clause[B.b](b.5, &b$entry)
  let w.6: evv = prim evv_enter(w.3, b.5)
  let u.7: unit = prim evv_set(w.6)
  let t.9: tobj = clause A.a(())
  let u.10: unit = prim evv_set(w.0)
  let n.11: int = unbox t.9
  decref t.9
  let u.92: unit = drop a.2
  let u.93: unit = drop b.5
  return n.11
}}
internal clause A.a fn a$entry(e.0: cev, u.1: unit) -> tobj {{
  let s.2: tobj = prim ev_take_state(e.0)
  decref e.0
  return s.2
}}
internal clause B.b fn b$entry(e.0: cev, u.1: unit) -> tobj {{
  decref e.0
  return ()
}}
",
        first.to_lowercase(),
        second.to_lowercase()
    )
}

/// エフェクトの番号はマシンの表のもので、コードは`e:<エフェクト>`の記号から読む。宣言の順の違う2つのプログラムで、
/// 同じ関数は同じコードを持ち、同じ結果を返し、`DATUM_EFFECT`の行だけが違う番号を持つ。
#[test]
fn effect_numbers_come_from_the_machine_table() {
    let ab = two_effects("A", "B");
    let ba = two_effects("B", "A");
    assert_eq!(clif(&ab, "f"), clif(&ba, "f"));
    for text in [&ab, &ba] {
        assert_eq!(crate::common::native::call(text, "f", &[]), [Arg::I64(1)]);
    }
    let bits = |text: &str| -> Vec<u64> {
        datum_rows(text, DATUM_EFFECT)
            .iter()
            .map(|row| row.bits)
            .collect()
    };
    // 値の行は、関数が初めて使った順 (`e:A`、`e:B`) に並ぶ
    assert_eq!(bits(&ab), [0, 1]);
    assert_eq!(bits(&ba), [1, 0]);
}

/// 根の基本操作のテストの記録。関数の結果と、関数が戻った後の根の表である。
type Roots = Option<(Word, Vec<Option<Word>>)>;

/// 代役。根1に即値を置いてから`probe`を呼び、結果と根の表を記録して、どれも手放す。
unsafe fn with_roots(ctx: *mut Ctx<Fast>, visit: &mut Visit<Roots>) {
    // SAFETY: 代役の中で、受け取った`ctx`で呼び、関数の呼び出しをまたいで`ctx`への参照を持たない。根の参照は
    // `take_roots`で代役に移り、代役が手放す
    let seen = unsafe {
        let set = (*ctx).root_set(1, Word::imm(9));
        let mut argv = [0u64];
        call_visited(ctx, visit, &mut argv);
        let result = Word::from_bits(argv[0] as usize);
        let roots = (*ctx).take_roots();
        let mut released = (*ctx).heap_mut().decref(result).is_ok();
        for &root in roots.iter().flatten() {
            released &= (*ctx).heap_mut().decref(root).is_ok();
        }
        (set.is_ok() && released).then_some((result, roots))
    };
    visit.record = seen;
}

/// `root_set`で置いた語を`root_get`が`dup`して返す。表は根の番号まで伸び、間の根は空のままで、代役の置いた根は残る。
#[test]
fn root_set_then_root_get_round_trips() {
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn probe() -> tobj {
  let a.0: obj = con (,) #0(#5, #6)
  let u.1: unit = prim root_set[3](a.0)
  let t.2: tobj = prim root_get[3]()
  return t.2
}
";
    let (finished, record) = visit(text, "probe", with_roots, None);
    finished.unwrap_or_else(|error| panic!("{error}"));
    let (result, roots) = record.expect("the stand-in released every reference");
    assert!(!result.is_imm(), "the pair is a heap object");
    assert_eq!(roots, [None, Some(Word::imm(9)), None, Some(result)]);
}

/// ネイティブのrunの誤りのfault。
fn fault_of(result: Result<eml_nrt::Finished, RuntimeError>) -> RunFault {
    match result {
        Err(RuntimeError::Fault { fault, .. }) => fault,
        other => panic!("the run ends with a fault: {other:?}"),
    }
}

/// 根の誤った読み書きでrunを終えるfault。表の外の根については、VMの誤りを`eml_vm`の
/// `roots::reading_an_empty_root_is_an_error` (根のない表での`root_get[3]`) が固定する。ほかの場合 (表の中の空の根と、
/// 置いてある根への`root_set`) を固定するのは`eml_rt`の`RootMisuse`だけで、VMも同じfaultを報告する。
const ROOT_MISUSE: RunFault = RunFault::Extern(Fault::Heap(HeapError::RootMisuse));

/// 空の根と表の外の根の`root_get`は`RootMisuse`でrunを終える。VMが固定するのは表の外の根の場合である (`ROOT_MISUSE`)。
#[test]
fn root_get_of_an_empty_root_ends_with_the_vm_error() {
    let text = "\
fn probe() -> tobj {
  let t.0: tobj = prim root_get[0]()
  return t.0
}
";
    // 根1だけを置いた表で、根0は表の中の空の根である
    let (finished, _) = visit(text, "probe", with_roots, None);
    assert_eq!(fault_of(finished), ROOT_MISUSE);
    // 根のない表では、根0は表の外である
    let (_, result) = native(
        "\
fn main() -> unit {
  let t.0: tobj = prim root_get[0]()
  decref t.0
  return ()
}
",
    );
    assert_eq!(
        result,
        Err(format!("{ROOT_MISUSE} in `<native code>`")),
        "the native run names no function, as every fatal heap error of an entry"
    );
}

/// 置いてある根への`root_set`は`RootMisuse`でrunを終える。VMのテストはこの場合を固定せず、`eml_rt`の`RootMisuse`が固定する (`ROOT_MISUSE`)。
#[test]
fn root_set_of_a_set_root_ends_with_the_vm_error() {
    let text = "\
fn probe() -> tobj {
  let u.0: unit = prim root_set[1](#2)
  return ()
}
";
    let (finished, _) = visit(text, "probe", with_roots, None);
    assert_eq!(fault_of(finished), ROOT_MISUSE);
    let (_, result) = native(
        "\
fn main() -> unit {
  let u.0: unit = prim root_set[0](#1)
  let u.1: unit = prim root_set[0](#2)
  return u.1
}
",
    );
    assert_eq!(result, Err(format!("{ROOT_MISUSE} in `<native code>`")));
}

/// `Counter`のhandlerを1つ入れて`evv`に積み、そのevidenceの`get`の節を`clause`で呼んで、結果を表示する
/// `main`。`clause`は入れた節の値`value`を受け取り、節の関数`get`はプログラムの残り`rest`が定義する。`put`の
/// エントリには捕獲を持つクロージャを入れる。静的な行を作らないので、静的なクロージャの行は`get`のエントリのもの
/// だけになる。
fn run_get(value: &str, rest: &str) -> String {
    format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  let s.1: tobj = box 41
  let e.2: ev lin = prim ev_new[Counter](s.1, w.0)
  let u.3: unit = prim ev_set_clause[Counter.get](e.2, {value})
  let k.10: tobj = box 0
  let p.11: tobj = closure put$captured(k.10)
  let u.12: unit = prim ev_set_clause[Counter.put](e.2, p.11)
  let w.4: evv = prim evv_enter(w.0, e.2)
  let u.5: unit = prim evv_set(w.4)
  let r.8: tobj = clause Counter.get(())
  let u.9: unit = prim evv_set(w.0)
  let u.13: unit = drop e.2
  tail call show_boxed(r.8)
}}
internal clause Counter.put fn put$captured(k.0: tobj, e.1: cev, x.2: tobj) -> unit {{
  decref k.0
  decref e.1
  decref x.2
  return ()
}}
{rest}{SHOW_BOXED}{SHOW}"
    )
}

/// handlerの状態の写しを返す、その場で動く`Counter.get`の節。
const GET: &str = "\
internal clause Counter.get fn get(e.0: cev, u.1: unit) -> tobj {
  let s.2: tobj = prim ev_take_state(e.0)
  dup s.2
  let u.3: unit = prim ev_set_state(e.0, s.2)
  decref e.0
  return s.2
}
";

/// その場で動く節は、それを呼ぶフレームの上で動き、その値が節の呼び出しの結果として戻る。handlerの節は静的な
/// クロージャで、そのタグは0である。
#[test]
fn an_in_place_clause_returns_its_value() {
    agree(&run_get("&get", GET));
}

/// underの節は、動く間`evv`を自分のhandlerの`evv`にする。だから中の節の呼び出しは、自分のhandler (状態2) でなく
/// 外側のhandler (状態1) を見つける。
#[test]
fn an_under_clause_runs_with_its_handlers_evv() {
    agree(&format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  let s.1: tobj = box 1
  let a.2: ev lin = prim ev_new[Counter](s.1, w.0)
  let u.90: unit = prim ev_set_clause[Counter.get](a.2, &get$entry)
  let u.91: unit = prim ev_set_clause[Counter.put](a.2, &put$entry)
  let w.3: evv = prim evv_enter(w.0, a.2)
  dup w.3
  let s.4: tobj = box 2
  let b.5: ev lin = prim ev_new[Counter](s.4, w.3)
  let u.6: unit = prim ev_set_clause[Counter.get](b.5, &under)
  let u.92: unit = prim ev_set_clause[Counter.put](b.5, &put$entry)
  let w.7: evv = prim evv_enter(w.3, b.5)
  let u.8: unit = prim evv_set(w.7)
  let r.11: tobj = clause Counter.get(())
  let u.12: unit = prim evv_set(w.0)
  let u.93: unit = drop a.2
  let u.94: unit = drop b.5
  tail call show_boxed(r.11)
}}
internal clause Counter.get fn under(e.0: cev, u.1: unit) -> tobj {{
  let w.2: evv = prim evv_get()
  let o.3: evv = prim ev_evv(e.0)
  let u.4: unit = prim evv_set(o.3)
  decref e.0
  let s.6: tobj = clause Counter.get(())
  let u.8: unit = prim evv_set(w.2)
  return s.6
}}
{SHOW_BOXED}{SHOW}{COUNTER_ENTRIES}"
    ));
}

/// 末尾の位置の節の呼び出しは、クロージャのエントリの末尾呼び出しである。この節のクロージャは値を1つ捕まえていて、
/// そのエントリは節の関数を呼ぶ前にそれをロードする。
#[test]
fn a_tail_clause_call() {
    agree(&format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  let n.1: obj = extern \"Prelude.Show Int.show\"(7)
  let k.2: tobj = closure captured(n.1)
  let e.3: ev lin = prim ev_new[Counter]((), w.0)
  let u.4: unit = prim ev_set_clause[Counter.get](e.3, k.2)
  let u.90: unit = prim ev_set_clause[Counter.put](e.3, &put$entry)
  let w.5: evv = prim evv_enter(w.0, e.3)
  let u.6: unit = prim evv_set(w.5)
  let r.9: tobj = call ask()
  let u.10: unit = prim evv_set(w.0)
  let u.11: unit = extern Prelude.println(r.9)
  let u.91: unit = drop e.3
  return u.11
}}
fn ask() -> tobj {{
  tail clause Counter.get(())
}}
internal clause Counter.get fn captured(n.0: tobj, e.1: cev, u.2: unit) -> tobj {{
  decref e.1
  return n.0
}}
{COUNTER_ENTRIES}"
    ));
}

/// 節の呼び出しのシグネチャは操作のReprを持つ。`float`の引数と結果はF64で渡り、捕まえた値はevidenceより前に来る。
#[test]
fn a_clause_with_a_float_operand() {
    agree(
        "\
effect Scale { scale(float) -> float }
fn main() -> unit {
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  let k.1: tobj = box 2.5
  let m.2: tobj = closure times(k.1)
  let e.3: ev lin = prim ev_new[Scale]((), w.0)
  let u.4: unit = prim ev_set_clause[Scale.scale](e.3, m.2)
  let w.5: evv = prim evv_enter(w.0, e.3)
  let u.6: unit = prim evv_set(w.5)
  let x.9: float = clause Scale.scale(4.0)
  let u.10: unit = prim evv_set(w.0)
  let s.11: obj = extern \"Prelude.Show Float.show\"(x.9)
  let u.12: unit = extern Prelude.println(s.11)
  let u.13: unit = drop e.3
  return u.12
}
internal clause Scale.scale fn times(k.0: tobj, e.1: cev, x.2: float) -> float {
  decref e.1
  let y.3: float = unbox k.0
  decref k.0
  let z.4: float = extern \"Prelude.Num Float.*\"(x.2, y.3)
  return z.4
}
",
    );
}

/// 節の呼び出しは`evv`からevidenceを探す。`Counter`のevidenceを2つ積み、その後に`Other`のevidenceを積む。
/// `clause Counter.get`は`Other`を飛ばして内側の`Counter` (状態2) を見つけ、`clause Other.ask`は`Other` (状態3) を
/// 見つける。ネイティブのコードはVMと同じに走る (docs/spec/lir.mdの「節の呼び出し」)。
#[test]
fn a_clause_finds_its_evidence() {
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
  let c.2: ev lin = prim ev_new[Counter](s.1, w.0)
  let u.90: unit = prim ev_set_clause[Counter.get](c.2, &get$entry)
  let u.91: unit = prim ev_set_clause[Counter.put](c.2, &put$entry)
  let w.3: evv = prim evv_enter(w.0, c.2)
  let s.4: tobj = box 2
  let d.5: ev lin = prim ev_new[Counter](s.4, w.0)
  let u.92: unit = prim ev_set_clause[Counter.get](d.5, &get$entry)
  let u.93: unit = prim ev_set_clause[Counter.put](d.5, &put$entry)
  let w.6: evv = prim evv_enter(w.3, d.5)
  let s.7: tobj = box 3
  let o.8: ev lin = prim ev_new[Other](s.7, w.0)
  let u.94: unit = prim ev_set_clause[Other.ask](o.8, &ask$entry)
  let w.9: evv = prim evv_enter(w.6, o.8)
  let u.10: unit = prim evv_set(w.9)
  let a.11: tobj = clause Counter.get(())
  let b.12: tobj = clause Other.ask(())
  let u.13: unit = prim evv_set(w.0)
  let p.14: unit = call show_boxed(a.11)
  let u.95: unit = drop c.2
  let u.96: unit = drop d.5
  let u.97: unit = drop o.8
  tail call show_boxed(b.12)
}}
{SHOW_BOXED}{SHOW}{COUNTER_ENTRIES}{OTHER_ENTRY}"
    );
    agree(&text);
    assert_eq!(native(&text).0, "2\n3\n");
}

/// 節のクロージャのタグは0で、`apply`で呼ばれることはない。静的なクロージャの`DATUM_FN`の行の`bits`も0である。
#[test]
fn a_static_clause_closure_has_tag_0() {
    let text = run_get("&g$entry", &GET.replace("fn get(", "fn g$entry("));
    let rows: Vec<u64> = datum_rows(&text, DATUM_FN)
        .iter()
        .map(|row| row.bits)
        .collect();
    assert_eq!(rows, [0]);
}

/// 戻らない操作が1つだけのエフェクトと、その節`quit`。節は`exit`でrunを終える。`exit`は中断を始めるので、節の呼び出しは
/// `abort`のバイトが立ったまま戻る。
const FAIL: &str = "\
effect Fail { never fail(tobj) -> never }
";
const QUIT: &str = "\
internal clause Fail.fail fn quit(e.0: cev, a.1: tobj) -> obj {
  decref e.0
  decref a.1
  let z.2: unit = extern Prelude.exit(3)
  let d.3: obj = prim dummy()
  return d.3
}
";

/// `Fail`のhandlerを入れ、`call`で`fail`を呼ぶ`main`。`main`は`call`をまたいで文字列を持つので、後始末がそれを
/// 手放さなければcensusが漏れを見つける。`call`の束縛は`bind`で、`fail`はプログラムの残り`rest`が定義する。
fn run_fail(bind: &str, rest: &str) -> String {
    format!(
        "{FAIL}\
fn main() -> unit {{
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  let e.1: ev lin = prim ev_new[Fail]((), w.0)
  let u.2: unit = prim ev_set_clause[Fail.fail](e.1, &quit)
  let w.3: evv = prim evv_enter(w.0, e.1)
  let u.4: unit = prim evv_set(w.3)
  let s.5: obj = extern \"Prelude.Show Int.show\"(5)
  dup s.5
  let u.6: unit = extern Prelude.println(s.5)
  {bind} = call fail()
  let u.9: unit = prim evv_set(w.0)
  let u.10: unit = extern Prelude.println(s.5)
  let u.13: unit = drop e.1
  return u.10
}}
{rest}{QUIT}"
    )
}

/// `agree`と同じく両方のエンジンで流し、ネイティブのrunが文字列を1回表示して、節の`exit`の終了コード3で終わったことを
/// 確かめる。
fn exits(text: &str) {
    agree(text);
    assert_eq!(native(text), ("5\n".to_string(), Ok(Some(3))));
}

/// `float`を返す関数の、戻らない操作の末尾の節の呼び出しは、末尾でない間接の呼び出しと、その後の中断の確かめである。
/// 冷たい側は関数の結果の仮の値を返す。
#[test]
fn a_tail_never_clause_in_a_float_function() {
    exits(&run_fail(
        "let x.11: float",
        "fn fail() -> float {\n  tail clause Fail.fail(())\n}\n",
    ));
}

/// 結果の葉が2つの関数でも同じである。冷たい側は葉ごとの仮の値を返す。
#[test]
fn a_tail_never_clause_in_a_function_returning_two_leaves() {
    exits(&run_fail(
        "let (x.11: float, n.12: int)",
        "fn fail() -> (float, int) {\n  tail clause Fail.fail(())\n}\n",
    ));
}

/// `float`に束縛した戻らない節の呼び出しは、1語の結果を捨てる。冷たい側はフレームが持つ文字列を手放し、ブロックの残りは
/// 仮の値で下ろす。
#[test]
fn a_never_clause_bound_to_a_float() {
    exits(&run_fail(
        "let x.11: float",
        "\
fn fail() -> float {
  let s.2: obj = extern \"Prelude.Show Int.show\"(6)
  let x.3: float = clause Fail.fail(())
  let u.4: unit = extern Prelude.println(s.2)
  let y.5: float = extern \"Prelude.Num Float.+\"(x.3, 1.0)
  return y.5
}
",
    ));
}

/// verifierを通さずに読んだ`text`のプログラムを読み込み、`load`のpanicの文を返す。
fn load_panic(text: &str) -> String {
    let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    let linked = eml_test_support::link(&program);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut machine = machine();
        let _ = machine.load(&program, &linked);
    }))
    .expect_err("the load panics");
    match panic.downcast::<String>() {
        Ok(message) => *message,
        Err(panic) => panic
            .downcast::<&str>()
            .map(|message| message.to_string())
            .unwrap_or_default(),
    }
}

/// 末尾の節の呼び出しで、操作の結果が呼ぶ側の結果と違うものは、どのビルドでも読み込みで断る。読み込みはどのビルドでも
/// verifierを走らせるので、そこで断る。互換のReprは同じKindを持つので、シグネチャの結果についての下ろしの`assert`が
/// 先に働くことは今はなく、2つ目の守りとして残る (docs/spec/lir.md の「verifier」)。
#[test]
fn release_check_a_tail_clause_result_mismatch_panics() {
    let message = load_panic(&format!(
        "{COUNTER}\
fn main() -> unit {{
  return ()
}}
fn f() -> float {{
  tail clause Counter.get(())
}}
"
    ));
    let expected = "invalid LIR at the native load";
    assert!(message.contains(expected), "{message}");
}

/// 引数が操作のものと違う節の関数は、どのビルドでも読み込みのverifierが断る。クロージャのエントリを定義する繰り返しの
/// `assert`は2つ目の守りとして残る。
#[test]
fn release_check_a_clause_entry_signature_mismatch_panics() {
    let message = load_panic(&format!(
        "{COUNTER}\
fn main() -> unit {{
  let w.0: evv = prim evv_get()
  let e.1: ev lin = prim ev_new[Counter]((), w.0)
  let u.2: unit = prim ev_set_clause[Counter.get](e.1, &g)
  let u.3: unit = prim ev_set_clause[Counter.put](e.1, &p)
  let u.4: unit = drop e.1
  return ()
}}
internal clause Counter.get fn g(e.0: cev, x.1: float) -> tobj {{
  decref e.0
  return ()
}}
internal clause Counter.put fn p(e.0: cev, x.1: tobj) -> unit {{
  decref e.0
  decref x.1
  return ()
}}
"
    ));
    let expected = "invalid LIR at the native load";
    assert!(message.contains(expected), "{message}");
}

/// 節の呼び出しは、エントリのクロージャを確かめずに操作のシグネチャで呼ぶので、読めない関数を入れる`ev_set_clause`は、
/// どのビルドでも読み込みのverifierが断る (docs/spec/lir.md の「verifier」)。
#[test]
fn release_check_an_unreadable_clause_entry_panics() {
    let message = load_panic(&format!(
        "{COUNTER}\
fn main() -> unit {{
  return ()
}}
fn f(c.0: tobj, w.1: evv) -> unit {{
  let e.2: ev lin = prim ev_new[Counter]((), w.1)
  let u.3: unit = prim ev_set_clause[Counter.get](e.2, c.0)
  let u.4: unit = drop e.2
  return ()
}}
"
    ));
    let expected = "invalid LIR at the native load";
    assert!(message.contains(expected), "{message}");
}

/// 入れていないエントリは即値を持ち、節の呼び出しはそれをクロージャとして読むので、ほかの最初の使いより前に
/// エントリをすべて入れていないevidenceは、どのビルドでも読み込みのverifierが断る (docs/spec/lir.md の「verifier」)。
#[test]
fn release_check_an_unfilled_entry_panics() {
    let message = load_panic(&format!(
        "{COUNTER}\
fn main() -> unit {{
  return ()
}}
fn f(w.0: evv) -> unit {{
  let e.1: ev lin = prim ev_new[Counter]((), w.0)
  let u.2: unit = prim ev_set_clause[Counter.get](e.1, &g)
  let u.3: unit = drop e.1
  return ()
}}
internal clause Counter.get fn g(e.0: cev, u.1: unit) -> tobj {{
  decref e.0
  return ()
}}
"
    ));
    let rule = "the `Counter.put` entry of `e.1` is not filled before `e.1` is used";
    let expected = format!("invalid LIR at the native load: {rule} in `f`");
    assert!(message.contains(&expected), "{message}");
}

/// `agree_source`と同じだが、先にコンパイルしたプログラムが着地の印を持つ巻き戻しの地点を持つことを確かめる。optimizeは
/// 本体と節を丸ごと見えるhandlerを消すので、そうなればテストは着地について何も確かめなくなるためである。
fn agree_landing(source: &str) {
    let program = eml_test_support::lir(source);
    let lands = eml_lir::unwind::points(&program, |_| true)
        .into_iter()
        .flatten()
        .any(|unwinds| {
            unwinds
                .unwrap_or_else(|error| panic!("{error}"))
                .points
                .iter()
                .any(|point| point.landing.is_some())
        });
    assert!(lands, "the compiled program has a landing point");
    agree_source(source);
}

/// EPの`prompt$`、`resume$`、`rewind$`。EPが出すものを写したもので、手で書くhandleはこれを呼ぶ。
const PROMPT: &str = "\
yields fn prompt$(ev.0: ev lin, ret.1: tobj poly, x.2: tobj poly) -> tobj poly {
  let w.3: evv = prim ev_take_evv(ev.0)
  let u.4: unit = prim evv_set(w.3)
  let s.5: int = prim yield_status(ev.0)
  switch s.5 { 0 -> b1, 1 -> b2, 2 -> b3, _ -> b4 }
b1:
  let st.6: tobj poly = prim ev_last_state(ev.0)
  let u.25: unit = drop ev.0
  tail yields apply ret.1(x.2, st.6)
b2:
  let u.8: unit = drop x.2
  let k.9: tobj poly = prim yield_cont_take()
  let c.10: tobj poly = prim yield_finish()
  let st.11: tobj poly = prim ev_last_state(ev.0)
  let h.26: rev = prim ev_retire(ev.0)
  let k.12: tobj poly = closure resume$(h.26, ret.1, k.9)
  tail yields apply c.10(k.12, st.11)
b3:
  let u.14: unit = drop ret.1
  let u.15: unit = drop x.2
  let c.16: tobj poly = prim yield_finish()
  let st.17: tobj poly = prim ev_last_state(ev.0)
  let u.27: unit = drop ev.0
  tail yields apply c.16(st.17)
b4:
  let u.19: unit = drop x.2
  let st.20: tobj poly = prim ev_last_state(ev.0)
  let k.21: tobj poly = prim yield_cont_take()
  let h.28: rev = prim ev_retire(ev.0)
  let k.22: tobj poly = closure rewind$(h.28, ret.1, st.20, k.21)
  let u.23: unit = prim yield_cont_set(k.22)
  let d.24: obj poly = prim dummy()
  return d.24
}
yields fn resume$(h.0: rev, ret.1: tobj poly, k.2: tobj poly, v.3: tobj poly, s.4: tobj poly) -> tobj poly {
  let e.5: ev lin = prim ev_renew(h.0, s.4)
  decref h.0
  let w.6: evv = prim evv_get()
  let w.7: evv = prim evv_enter(w.6, e.5)
  let u.8: unit = prim evv_set(w.7)
  let x.9: tobj poly = yields apply k.2(v.3) prompt e.5
  tail call prompt$(e.5, ret.1, x.9)
}
yields fn rewind$(h.0: rev, ret.1: tobj poly, s.2: tobj poly, k.3: tobj poly, v.4: tobj poly) -> tobj poly {
  tail call resume$(h.0, ret.1, k.3, v.4, s.2)
}
";

/// `Fail` (`FAIL`) のエントリ`fail$entry`は、受け取った引数を捕まえた節のクロージャ`fail$k`で`yield_abort`し、
/// handlerへ中断する。`fail$k`はその引数を結果にする。`ret`はhandleの`return`の節である。
const FAIL_ABORTS: &str = "\
internal clause Fail.fail fn fail$entry(e.0: cev, m.1: tobj) -> obj {
  let c.2: tobj = closure fail$k(m.1)
  let u.3: unit = prim yield_abort(e.0, c.2)
  let d.4: obj = prim dummy()
  return d.4
}
fn fail$k(m.0: tobj, st.1: tobj) -> tobj {
  decref st.1
  return m.0
}
fn ret(x.0: tobj, st.1: tobj) -> tobj {
  decref st.1
  return x.0
}
";

/// `never`の操作の中断はhandleの着地の印を持つ`apply`に着地し、`prompt$`がhandlerの節を走らせて、その値がhandleの
/// 値になる。中断しないhandleは本体の値を返す。
#[test]
fn an_abort_lands_and_the_clause_returns() {
    agree_landing(
        "\
effect Fail where
  never fail : String -> a

check : Int -> <Fail> Int
check n = if n > 0 then n else fail \"not positive\"

safe : Int -> String
safe n =
  handle show (check n) with
    | ctl fail message -> \"error: \" ++ message

main : Unit -> <IO> Unit
main () =
  println (safe 3)
  println (safe 0)
",
    );
}

/// handleとエントリの間の3つのフレームは、どれも呼び出しをまたいで文字列と生きたtokenを持つ。中断はそれぞれの後始末を
/// 通ってから着地し、着地したフレームは持っていた文字列を使い続ける。後始末が参照かtokenの箱を残せば、censusが漏れを
/// 見つける。
#[test]
fn an_abort_unwinds_three_frames_and_lands() {
    let text = format!(
        "\
layout P {{ P(obj, int) }}
{FAIL}\
fn main() -> unit {{
  let s.0: obj = extern \"Prelude.Show Int.show\"(9)
  let w.1: evv = prim evv_get()
  dup w.1
  let ev.2: ev lin = prim ev_new[Fail]((), w.1)
  let u.3: unit = prim ev_set_clause[Fail.fail](ev.2, &fail$entry)
  let w.4: evv = prim evv_enter(w.1, ev.2)
  let u.5: unit = prim evv_set(w.4)
  let x.6: tobj = yields apply &body(()) prompt ev.2
  let r.7: obj = call prompt$(ev.2, &ret, x.6)
  let u.8: unit = extern Prelude.println(s.0)
  let u.9: unit = extern Prelude.println(r.7)
  return u.9
}}
fn body(u.0: tobj) -> tobj {{
  decref u.0
  let a.1: obj = extern \"Prelude.Show Int.show\"(1)
  let p.2: obj = con P #0(a.1, 2)
  let n.3: int = call f1(p.2)
  let b.4: tobj = box n.3
  return b.4
}}
fn f1(x.0: obj) -> int {{
  unpack x.0 P #0(s.1: obj, n.2: int)
  let r.3: token = release x.0 P #0(s.1, _)
  let a.4: obj = extern \"Prelude.Show Int.show\"(n.2)
  let y.5: obj = con P #0(a.4, n.2)
  let m.6: int = call f2(y.5)
  let d.7: obj = con P #0(s.1, m.6) reuse r.3 in place
  decref d.7
  return m.6
}}
fn f2(x.0: obj) -> int {{
  unpack x.0 P #0(s.1: obj, n.2: int)
  let r.3: token = release x.0 P #0(s.1, _)
  let a.4: obj = extern \"Prelude.Show Int.show\"(n.2)
  let y.5: obj = con P #0(a.4, n.2)
  let m.6: int = call f3(y.5)
  let d.7: obj = con P #0(s.1, m.6) reuse r.3 in place
  decref d.7
  return m.6
}}
fn f3(x.0: obj) -> int {{
  unpack x.0 P #0(s.1: obj, n.2: int)
  let r.3: token = release x.0 P #0(s.1, _)
  let m.6: obj = extern \"Prelude.Show Int.show\"(n.2)
  let k.7: int = clause Fail.fail(m.6)
  let d.8: obj = con P #0(s.1, k.7) reuse r.3 in place
  decref d.8
  return k.7
}}
{FAIL_ABORTS}{PROMPT}"
    );
    let program = program(&text);
    let points = eml_lir::unwind::points(&program, |_| true);
    let points_of = |name: &str| {
        let index = program
            .functions
            .iter()
            .position(|function| function.name == name)
            .unwrap_or_else(|| panic!("no function `{name}`"));
        points[index]
            .as_ref()
            .expect("every function is wanted")
            .as_ref()
            .unwrap_or_else(|error| panic!("{error}"))
            .points
            .clone()
    };
    for name in ["f1", "f2", "f3"] {
        assert!(
            points_of(name)
                .iter()
                .any(|point| !point.owned.is_empty() && !point.tokens.is_empty()),
            "`{name}` holds a reference and a token across a point"
        );
    }
    assert!(
        points_of("main")
            .iter()
            .any(|point| point.landing.is_some() && !point.owned.is_empty()),
        "`main` lands holding a reference"
    );
    agree(&text);
    assert_eq!(native(&text), ("9\n2\n".to_string(), Ok(None)));
}

/// 再開できるyieldは`abort`のバイトを立てないので、3つのフレームを熱い道で戻り、それぞれのフレームのLIRの`yielding`の
/// 確かめが継続を組み立てる。節はそれを再開し、再開の後も続く。
#[test]
fn a_resumable_yield_passes_three_frames() {
    agree_landing(
        "\
effect Ask where
  ask : Unit -> Int

frames : Int -> <Ask, Panic> Int
frames n = if n == 0 then ask () else frames (n - 1) * 2 + n

main : Unit -> <IO, Panic> Unit
main () =
  let total =
    handle frames 3 with
      | ctl ask () k ->
          println \"ask\"
          let r = k 10
          println \"resumed\"
          r + 1
  println (show total)
",
    );
}

/// 何度も呼べる継続は、呼ぶたびに捕まえた部分を写して走らせる。
#[test]
fn a_multi_shot_continuation() {
    agree_landing(
        "\
effect Choice where
  multi choose : Unit -> Bool

bits : Int -> <Choice, Panic> Int
bits n = if n == 0 then 0 else (if choose () then 1 else 0) + 2 * bits (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let every =
    handle show (bits 2) with
      | ctl choose () k -> k False ++ \", \" ++ k True
  println every
",
    );
}

/// どの節でも継続を再開し、`return`の節を通って戻るhandleは、物体を残さない。runの終わりのcensusは、evidence、その退い
/// た写し、新しくしたevidenceの参照の欠けや余りを見つける。
#[test]
fn a_handler_leaves_no_object_behind() {
    agree_source(
        "\
effect Counter where
  tick : Unit -> Int

count : Int -> <Counter, Panic> Int
count n = if n == 0 then 0 else tick () + count (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let total =
    handle count 3 with
      | ctl tick () k -> k 1 + 1
      | return x -> x * 10
  println (show total)
",
    );
}

/// `run`自身の`log`は内側のhandlerに届き、`mask`の包みを通る`action`の`log`は内側のhandlerを飛ばして外側に届く。
#[test]
fn a_masked_perform_skips_the_inner_handler() {
    agree_landing(
        "\
effect Log where
  log : String -> Unit

run : (Unit -> <e> a) -> <Log | e> a
run action =
  log \"own\"
  action ()

tagged : String -> (Unit -> <Log, IO | e> a) -> <IO | e> a
tagged tag action =
  handle action () with
    | ctl log s k ->
      println \"\\{tag}: \\{s}\"
      let r = k ()
      println \"\\{tag} resumed\"
      r

main : Unit -> <IO> Unit
main () =
  use tagged \"outer\"
  use tagged \"inner\"
  run (fn () -> log \"masked\")
",
    );
}

/// 外側のhandlerへの中断が内側のhandleのフレームを通ると、そのフレームは内側のhandlerの状態を取り出して手放してから
/// 抜ける。VMと同じ順である。この`prompt$`は`evv`を戻さないので、内側のevidenceは`evv`の中で生きたままである。代役は、
/// `probe`が戻った後、`evv`を戻す前に、状態の物体が生きていないことを確かめる。
#[test]
fn an_abort_through_a_handler_frame_releases_its_state() {
    let text = "\
effect Fail { never fail(unit) -> never }
effect Inner { op(unit) -> tobj }
fn make() -> obj {
  let s.0: obj = extern \"Prelude.Show Int.show\"(12345)
  return s.0
}
fn probe(s.0: obj) -> unit {
  let w.1: evv = prim evv_get()
  dup w.1
  let ev.2: ev lin = prim ev_new[Fail]((), w.1)
  let u.3: unit = prim ev_set_clause[Fail.fail](ev.2, &fail$entry)
  let w.4: evv = prim evv_enter(w.1, ev.2)
  let u.5: unit = prim evv_set(w.4)
  let x.6: tobj = yields apply &inner(s.0) prompt ev.2
  let r.7: tobj = call prompt$(ev.2, x.6)
  decref r.7
  return ()
}
yields fn inner(s.0: tobj) -> tobj {
  let w.1: evv = prim evv_get()
  dup w.1
  let ev.2: ev lin = prim ev_new[Inner](s.0, w.1)
  let u.3: unit = prim ev_set_clause[Inner.op](ev.2, &op$entry)
  let w.4: evv = prim evv_enter(w.1, ev.2)
  let u.5: unit = prim evv_set(w.4)
  let x.6: tobj = yields apply &fail(()) prompt ev.2
  tail call prompt$(ev.2, x.6)
}
yields fn fail(u.0: tobj) -> tobj {
  decref u.0
  tail clause Fail.fail(())
}
internal clause Fail.fail fn fail$entry(e.0: cev, u.1: unit) -> obj {
  let u.2: unit = prim yield_abort(e.0, ())
  let d.3: obj = prim dummy()
  return d.3
}
internal clause Inner.op fn op$entry(e.0: cev, u.1: unit) -> tobj {
  decref e.0
  return ()
}
fn prompt$(ev.0: ev lin, x.1: tobj) -> tobj {
  let u.2: unit = drop x.1
  let c.3: tobj = prim yield_finish()
  decref c.3
  let u.4: unit = drop ev.0
  return ()
}
";
    let (finished, record) = visit_each(text, &["make", "probe"], state_released, None);
    finished.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(record, Some(false), "the state is not live after the abort");
    // 内側のhandlerが同じ`Fail`でも、着地は効果でなくevidenceで比べるので、中断は内側のフレームを外して外側に着地する。
    // `fail`は`evv_mask[Fail]`で内側のhandlerを飛ばし、外側のevidenceへ中断する
    let same = text
        .replace(
            "prim ev_new[Inner](s.0, w.1)\n  let u.3: unit = prim ev_set_clause[Inner.op](ev.2, &op$entry)",
            "prim ev_new[Fail](s.0, w.1)\n  let u.3: unit = prim ev_set_clause[Fail.fail](ev.2, &fail$entry)",
        )
        .replace(
            "  decref u.0\n  tail clause Fail.fail(())",
            "  decref u.0\n  let w.3: evv = prim evv_get()\n  let m.4: evv = prim evv_mask[Fail](w.3)\n  decref w.3\n  let u.5: unit = prim evv_set(m.4)\n  tail clause Fail.fail(())",
        );
    assert!(same.contains("ev_new[Fail](s.0") && same.contains("evv_mask[Fail]"));
    let (finished, record) = visit_each(&same, &["make", "probe"], state_released, None);
    finished.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        record,
        Some(false),
        "the inner handler of the same effect is left and its state released"
    );
}

/// 代役。`make`の文字列を内側のhandlerの状態にして`probe`を呼び、戻った後に、その文字列が生きている物体の中にあるかを
/// 記録する。その後`evv`を`probe`の前のものに戻す。
unsafe fn state_released(ctx: *mut Ctx<Fast>, visit: &mut Visits<Option<bool>>) {
    // SAFETY: 代役の中で、受け取った`ctx`で呼び、関数の呼び出しをまたいで`ctx`への参照を持たない。文字列の参照は
    // `probe`が消費し、`evv_get`の参照は`evv_set`が消費する
    let seen = unsafe {
        let saved = (*ctx).evv_get();
        let mut argv = [0u64];
        call_visited_at(ctx, visit, 0, &mut argv);
        let state = Word::from_bits(argv[0] as usize);
        let mut argv = [state.bits() as u64];
        call_visited_at(ctx, visit, 1, &mut argv);
        let heap = (*ctx).heap_mut();
        heap.reclaim();
        let live = heap.live_objects().contains(&state);
        let restored = saved.and_then(|saved| (*ctx).evv_set(saved));
        restored.is_ok().then_some(live)
    };
    visit.record = seen;
}

/// `never`の操作のエントリが`prim yield_abort`で中断し、それを呼んだ`never`の節の呼び出しの`abort`のバイトの確かめが
/// 後始末へ進む。末尾でない位置では後始末がフレームの持つ文字列を手放し、末尾の位置では仮の値を返す。どちらもhandleに
/// 着地する。
#[test]
fn an_abort_through_a_never_clause_entry_across_a_never_clause_call() {
    let text = format!(
        "{FAIL}\
fn main() -> unit {{
  let a.0: obj = call run(&in_place)
  let u.1: unit = extern Prelude.println(a.0)
  let b.2: obj = call run(&in_tail)
  let u.3: unit = extern Prelude.println(b.2)
  return u.3
}}
fn run(b.0: tobj) -> obj {{
  let w.1: evv = prim evv_get()
  dup w.1
  let ev.2: ev lin = prim ev_new[Fail]((), w.1)
  let u.3: unit = prim ev_set_clause[Fail.fail](ev.2, &fail$entry)
  let w.4: evv = prim evv_enter(w.1, ev.2)
  let u.5: unit = prim evv_set(w.4)
  let x.6: tobj = yields apply b.0(()) prompt ev.2
  tail call prompt$(ev.2, &ret, x.6)
}}
yields fn in_place(u.0: tobj) -> tobj {{
  decref u.0
  let s.3: obj = const \"in place\"
  let t.4: obj = extern \"Prelude.Show Int.show\"(4)
  let x.5: int = clause Fail.fail(s.3)
  let u.6: unit = extern Prelude.println(t.4)
  let b.7: tobj = box x.5
  return b.7
}}
yields fn in_tail(u.0: tobj) -> tobj {{
  decref u.0
  let s.3: obj = const \"tail\"
  tail clause Fail.fail(s.3)
}}
{FAIL_ABORTS}{PROMPT}"
    );
    agree(&text);
    assert_eq!(native(&text), ("in place\ntail\n".to_string(), Ok(None)));
}

/// 着地の印を持つ`apply`の後には`prompt$`の呼び出しが続かなければならない。`abort`のバイトだけを確かめる呼び出しの
/// 後のコードはこれに頼るので、どのビルドでも読み込みのverifierが断る。下ろしの`assert`は2つ目の守りとして残る
/// (docs/spec/native.md の「巻き戻しの地点」)。
#[test]
fn release_check_a_marked_apply_not_followed_by_prompt_panics() {
    let message = load_panic(&format!(
        "{COUNTER}\
fn main() -> unit {{
  return ()
}}
yields fn g(u.0: tobj) -> tobj {{
  return u.0
}}
fn f(w.2: evv) -> tobj {{
  let ev.0: ev lin = prim ev_new[Counter]((), w.2)
  let u.3: unit = prim ev_set_clause[Counter.get](ev.0, &get$entry)
  let u.4: unit = prim ev_set_clause[Counter.put](ev.0, &put$entry)
  let x.1: tobj = yields apply &g(()) prompt ev.0
  let u.5: unit = drop ev.0
  return x.1
}}
{COUNTER_ENTRIES}"
    ));
    let rule = "the apply with the prompt marker `ev.0` is not followed by `prompt$`";
    let expected = format!("invalid LIR at the native load: {rule} in `f`");
    assert!(message.contains(&expected), "{message}");
}

/// 読み込みは、関数ごとの検査だけでなく、プログラムの全体にかかる規則 (`clause`の印の関数のABI) もどのビルドでも
/// 断る (docs/spec/lir.md の「verifier」)。
#[test]
fn release_check_a_program_level_rule_is_refused() {
    let message = load_panic(&format!(
        "{COUNTER}\
fn main() -> unit {{
  return ()
}}
internal clause Counter.get fn g(e.0: cev, x.1: float) -> tobj {{
  decref e.0
  return ()
}}
"
    ));
    assert!(
        message.contains("invalid LIR at the native load"),
        "{message}"
    );
}
