//! 手書きのLIRを検査モードのネイティブのコードで流す。各監査の失敗、runの終わりのリークの確かめ、拒まれるフレームを
//! 通る中断、検査モードのコードが数える4つの数を確かめる (docs/spec/native.mdの「検査モード」)。verifierが拒むLIRは
//! `Trusted::unverified`で包む (`native_unverified`)。どのテストも、その誤りか数をリテラルで固定する。
#![cfg(not(miri))]
use eml_lir::Trusted;
use eml_native::NativeMachine;
use eml_nrt::{Finished, RunConfig, RunFault, RuntimeError};
use eml_rt::{Fault, HeapError};

use crate::common::native::{
    host, native, native_checked, native_options, native_unverified, program, unverified,
};

#[test]
fn a_leak() {
    // `f`は作った物体を手放さずに捨てる。verifierはこれを断り、runの終わりがリークを報告する
    let text = "\
layout P { P(int) }
fn entry$main() -> unit {
  let u.0: unit = call f(1)
  return u.0
}
fn f(n.0: int) -> unit {
  let d.1: obj = con P #0(n.0)
  return ()
}
";
    let (_, result) = native_unverified(text);
    assert_eq!(
        result.unwrap_err(),
        RuntimeError::Leak(vec![("Data".to_string(), 1)])
    );
}

/// 物体を1つ確保して手放すプログラム。検査モードのrunは、終わりの検査をすべて通る。
const CLEAN: &str = "\
layout P { P(int) }
fn entry$main() -> unit {
  let u.0: unit = call f(1)
  return u.0
}
fn f(n.0: int) -> unit {
  let d.1: obj = con P #0(n.0)
  decref d.1
  return ()
}
";

/// `first`を検査モードの1つのマシンで2回流し、それから`CLEAN`を同じ`RunConfig`で`eml_native::execute`に通す。3つのrunは
/// 1つの`Control`を分け合う。
fn run_twice(first: &Trusted) -> [Result<Finished, RuntimeError>; 3] {
    let config = RunConfig::default().with_debug_heap(true);
    let mut machine = NativeMachine::new(&config, host());
    let loaded = machine.load(first, &eml_test_support::link(first)).unwrap();
    let a = machine.run(loaded, native_options());
    let b = machine.run(loaded, native_options());
    machine.close();
    let clean = program(CLEAN);
    let c = eml_native::execute(
        &clean,
        &eml_test_support::link(&clean),
        &config,
        host(),
        native_options(),
    );
    [a, b, c]
}

#[test]
fn a_run_after_a_leak_reports_its_own_leak() {
    // runはどれも自分の所有者の表で数えるので、1回目のrunのリークを、2回目のrunも、同じ`RunConfig`のrunも数え直さない
    let text = "\
layout P { P(int) }
fn entry$main() -> unit {
  let u.0: unit = call f(1)
  return u.0
}
fn f(n.0: int) -> unit {
  let d.1: obj = con P #0(n.0)
  return ()
}
";
    let [a, b, c] = run_twice(&unverified(text));
    let leak = RuntimeError::Leak(vec![("Data".to_string(), 1)]);
    assert_eq!(a.unwrap_err(), leak);
    assert_eq!(b.unwrap_err(), leak);
    assert!(c.is_ok(), "{c:?}");
}

const TRY_PANIC: &str = "\
fn boom(p.0: unit) -> tobj {
  let m.1: obj = const \"boom\"
  let r.2: tobj = extern Prelude.panic(m.1)
  return r.2
}
fn try_panic(f.0: tobj, c.1: tobj) -> tobj {
  let w.2: evv = prim evv_get()
  dup w.2
  let ev.3: ev lin = evidence Prelude.Panic(c.1, w.2) [&panicked$entry]
  let w.5: evv = prim evv_enter(w.2, ev.3)
  let u.6: unit = prim evv_set(w.5)
  let x.7: tobj poly = yields apply f.0(()) prompt ev.3
  tail call prompt$(ev.3, &return$, x.7)
}
clause Prelude.Panic.panicked fn panicked$entry(e.0: cev, m.1: obj) -> obj poly {
  decref m.1
  let u.2: unit = prim yield_abort(e.0, &panicked$clause)
  let d.3: obj poly = prim dummy()
  return d.3
}
fn panicked$clause(c.0: tobj) -> tobj {
  return c.0
}
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
fn return$(r.0: tobj, s.1: unit) -> tobj {
  return r.0
}
";

/// `main`が関数`leaky`で`try_panic`を呼び、handlerが戻った後に出力するプログラム。
fn through_leaky(effects: &str, leaky: &str) -> String {
    format!(
        "\
effect Prelude.Panic {{ never panicked(obj) -> never }}
{effects}fn main() -> unit {{
  let c.0: tobj = box 0
  let r.1: tobj = call try_panic(&leaky, c.0)
  decref r.1
  let s.2: obj = const \"after\"
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}}
{leaky}{TRY_PANIC}"
    )
}

/// verifierが拒む関数のフレームは、中断が通っても何も手放さない (docs/spec/native.mdの「検査モード」)。中断は正しい
/// 呼ぶ側のhandlerに着地し、フレームが所有していた文字列がrunのリークになる。
#[test]
fn an_abort_through_a_refused_frame_releases_nothing() {
    let text = through_leaky(
        "",
        "\
fn leaky(p.0: unit) -> tobj {
  let s.1: obj = extern \"Prelude.Show Int.show\"(7)
  let r.2: tobj = call boom(())
  return r.2
}
",
    );
    assert_eq!(
        native_unverified(&text),
        (
            "after\n".to_string(),
            Err(RuntimeError::Leak(vec![("String".to_string(), 1)]))
        )
    );
}

/// 拒まれる関数の印のある`apply`を通っても同じである。中断はその着地の地点を通り、その行は着地の印を持ち、
/// 何も手放さない。リークは、文字列と、フレームのevidenceとそれが届く物体 (5つの`Data`の物体) である。
#[test]
fn an_abort_through_a_marked_apply_of_a_refused_frame_releases_nothing() {
    let text = through_leaky(
        "effect Land { land(unit) -> tobj }\n",
        "\
yields fn leaky(p.0: unit) -> tobj {
  let s.1: obj = extern \"Prelude.Show Int.show\"(7)
  let w.2: evv = prim evv_get()
  let ev.3: ev lin = evidence Land((), w.2) [&land$entry]
  let x.4: tobj poly = yields apply &boom(()) prompt ev.3
  tail call prompt$(ev.3, &return$, x.4)
}
internal clause Land.land fn land$entry(e.0: cev, u.1: unit) -> tobj {
  decref e.0
  return ()
}
",
    );
    assert_eq!(
        native_unverified(&text),
        (
            "after\n".to_string(),
            Err(RuntimeError::Leak(vec![
                ("Data".to_string(), 5),
                ("String".to_string(), 1)
            ]))
        )
    );
}

#[test]
fn the_counters_follow_the_statements() {
    let text = "\
layout P { P(obj, int) }
fn f(x.0: tobj) -> tobj {
  let z.1: unit = extern Prelude.exit(3)
  return x.0
}
fn g(x.0: obj) -> obj {
  unpack x.0 P #0(s.1: obj, n.2: int)
  let r.3: token = release x.0 P #0(s.1, _)
  let d.4: obj = con P #0(s.1, n.2) reuse r.3 in place
  let a.5: tobj = box n.2
  let b.6: tobj = box 2.5
  let m.7: int = unbox a.5
  let w.8: float = unbox b.6
  let k.9: int = unbox a.5
  decref b.6
  let t.10: obj = extern \"Prelude.Show Int.show\"(m.7)
  let r.11: tobj = apply &f(a.5, t.10)
  decref r.11
  return d.4
}
fn entry$main() -> unit {
  let a.0: obj = extern \"Prelude.Show Int.show\"(1)
  let p.1: obj = con P #0(a.0, 2)
  let q.2: obj = call g(p.1)
  decref q.2
  return ()
}
";
    let (_, result) = native_checked(text);
    let finished = result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(finished.exit, Some(3));
    let stats = finished.stats;
    let counters = [
        stats.boxes,
        stats.unboxes,
        stats.reuses,
        stats.unwound_frames,
    ];
    // `g`は2回`box`し、3回`unbox`し、tokenを1回その場でreuseする。`exit`からの中断は4つのフレーム、`f`、補助の関数
    // `eml$apply$2`、`g`、`entry$main`を巻き戻す
    assert_eq!(
        counters,
        [2, 3, 1, 4],
        "boxes, unboxes, reuses, unwound_frames"
    );
}

#[test]
fn a_clause_call_passes_its_audit() {
    // 節の呼び出しの`code`の監査は、節の関数のエントリを通す。呼び出しの所の操作の署名の語は、imageのそのエントリの行の
    // 語である
    let text = "\
effect Counter { get(unit) -> tobj }
fn entry$main() -> unit {
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  let s.1: tobj = box 7
  let c.2: ev lin = evidence Counter(s.1, w.0) [&get$entry]
  let w.3: evv = prim evv_enter(w.0, c.2)
  let u.4: unit = prim evv_set(w.3)
  let s.5: tobj = clause Counter.get(())
  let u.6: unit = prim evv_set(w.0)
  let u.7: unit = drop c.2
  let n.8: int = unbox s.5
  decref s.5
  let t.9: obj = extern \"Prelude.Show Int.show\"(n.8)
  let u.10: unit = extern Prelude.println(t.9)
  return u.10
}
internal clause Counter.get fn get$entry(e.0: cev, u.1: unit) -> tobj {
  let s.2: tobj = prim ev_take_state(e.0)
  decref e.0
  return s.2
}
";
    let (out, result) = native_checked(text);
    assert_eq!(out, "7\n");
    assert_eq!(result.map(|finished| finished.exit), Ok(None));
}

#[test]
fn a_wide_clause_call_passes_its_audit() {
    // 操作が47個の`int`の引数と最後の`float`を持つ節の呼び出し。エントリの署名は添字48に`float`を持ち、`sig_word`は
    // これを書けないので、呼び出しの所はその語をデータの記号`w:`からロードし、エントリの行も同じimageだけの語を持つ。
    // 速いマシンも同じプログラムを読み込んで走らせる
    let ints: Vec<String> = (1..=47).map(|i| format!("a.{i}: int")).collect();
    let args: Vec<String> = (1..=47).map(|i| i.to_string()).collect();
    let text = format!(
        "\
effect Wide {{ op({}, float) -> tobj }}
fn entry$main() -> unit {{
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  let c.1: ev lin = evidence Wide((), w.0) [&op$entry]
  let w.2: evv = prim evv_enter(w.0, c.1)
  let u.3: unit = prim evv_set(w.2)
  let r.4: tobj = clause Wide.op({}, 2.5)
  let u.5: unit = prim evv_set(w.0)
  let u.6: unit = drop c.1
  let x.7: float = unbox r.4
  decref r.4
  let t.8: obj = extern \"Prelude.Show Float.show\"(x.7)
  let u.9: unit = extern Prelude.println(t.8)
  return u.9
}}
internal clause Wide.op fn op$entry(e.0: cev, {}, f.48: float) -> tobj {{
  decref e.0
  let b.49: tobj = box f.48
  return b.49
}}
",
        vec!["int"; 47].join(", "),
        args.join(", "),
        ints.join(", "),
    );
    let (out, result) = native_checked(&text);
    assert_eq!(out, "2.5\n");
    assert_eq!(result.map(|finished| finished.exit), Ok(None));
    assert_eq!(native(&text), ("2.5\n".to_string(), Ok(None)));
}

/// 監査の失敗で終わるrun。ネイティブでは失敗の手順を通るので、AddressSanitizerのジョブは`fatal::`を飛ばす
/// (docs/implementation/testing.mdの「`eml_native` の検証」)。
mod fatal {
    use super::*;

    fn heap(error: HeapError) -> RunFault {
        RunFault::Extern(Fault::Heap(error))
    }

    /// 失敗したrunのfault。関数と位置は除く。
    fn fault((_, result): (String, Result<Finished, RuntimeError>)) -> RunFault {
        match result {
            Err(RuntimeError::Fault { fault, .. }) => fault,
            Err(error) => panic!("not a fault: {error}"),
            Ok(_) => panic!("the run did not fail"),
        }
    }

    /// `entry$main`が`f(1)`を呼んでその結果を捨てるプログラム。`f`の本体は`body`である。
    fn calling_f(layouts: &str, ret: &str, body: &str) -> String {
        format!(
            "{layouts}fn entry$main() -> unit {{\n  let r.0: {ret} = call f(1)\n  return ()\n}}\nfn f(n.0: int) -> {ret} {{\n{body}}}\n"
        )
    }

    #[test]
    fn a_run_after_a_fatal_error_counts_from_zero() {
        // `d.1`の2回目の`decref`が失敗の手順に入るとき`k.2`はまだ生きているので、落としたヒープは数える物体を持っていた
        let text = calling_f(
            "layout P { P(int, int) }\n",
            "unit",
            "  let k.2: obj = con P #0(n.0, n.0)\n  let d.1: obj = con P #0(n.0, n.0)\n  decref d.1\n  decref d.1\n  decref k.2\n  return ()\n",
        );
        let [a, b, c] = run_twice(&unverified(&text));
        assert_eq!(fault((String::new(), a)), heap(HeapError::UseAfterFree));
        assert_eq!(fault((String::new(), b)), heap(HeapError::UseAfterFree));
        assert!(c.is_ok(), "{c:?}");
    }

    #[test]
    fn a_dup_of_a_lin_object() {
        // V1は`lin`の変数の`dup`を断る。`dup`の監査は物体の`lin`のビットを断る
        let text = calling_f(
            "layout Box { Box(int) }\n",
            "unit",
            "  let b.1: obj lin = con Box #0(n.0)\n  dup b.1\n  decref b.1\n  decref b.1\n  return ()\n",
        );
        assert_eq!(fault(native_unverified(&text)), heap(HeapError::LinDup));
    }

    #[test]
    fn a_second_decref() {
        // `d.1`の2回目の`decref`は解放したブロックに当たる。`decref`の監査はその前置きを読む
        let text = calling_f(
            "layout P { P(int, int) }\n",
            "unit",
            "  let d.1: obj = con P #0(n.0, n.0)\n  decref d.1\n  decref d.1\n  return ()\n",
        );
        assert_eq!(
            fault(native_unverified(&text)),
            heap(HeapError::UseAfterFree)
        );
    }

    #[test]
    fn an_unpack_of_a_freed_box() {
        // `unpack`の`data`の監査が、解放したブロックのヘッダを読む
        let text = calling_f(
            "layout P { P(int, int) }\n",
            "int",
            "  let d.1: obj = con P #0(n.0, n.0)\n  decref d.1\n  unpack d.1 P #0(a.2: int, b.3: int)\n  return a.2\n",
        );
        assert_eq!(
            fault(native_unverified(&text)),
            heap(HeapError::UseAfterFree)
        );
    }

    #[test]
    fn a_switch_on_a_freed_box() {
        // インラインのタグのロードの前の`header`の監査が、解放したブロックを読む
        let text = calling_f(
            "layout T { A(int), B(int) }\n",
            "int",
            "  let d.1: obj = con T #0(n.0)\n  decref d.1\n  switch d.1 T { #0 -> b1, #1 -> b2 }\nb1:\n  return 0\nb2:\n  return 1\n",
        );
        assert_eq!(
            fault(native_unverified(&text)),
            heap(HeapError::UseAfterFree)
        );
    }

    #[test]
    fn a_float_unbox_of_an_int_box() {
        // verifierは箱が持つ値を追わない。`float`の`unbox`の`box`の監査は、ダミーでない`BigInt`の物体を断る
        let text = "\
fn entry$main() -> unit {
  let b.0: tobj = box 4611686018427387904
  let x.1: float = unbox b.0
  decref b.0
  return ()
}
";
        assert_eq!(fault(native_checked(text)), heap(HeapError::WrongKind));
    }

    #[test]
    fn a_release_that_names_another_constructor() {
        // `release`の`data`の監査は、値を作ったのでない構築子について、VMの内部の誤りを報告する
        let text = "\
layout T { A(int), B(int, int) }
fn entry$main() -> unit {
  let d.0: obj = con T #0(1)
  let u.1: unit = call f(d.0)
  return u.1
}
fn f(x.0: obj) -> unit {
  let r.1: token = release x.0 T #1(_, _)
  drop_token r.1
  return ()
}
";
        assert_eq!(
            fault(native_unverified(text)),
            RunFault::Internal(
                "a release names a tag and number of fields the value does not have"
            )
        );
    }

    /// `field`から作った`C`の箱を`()`に適用するプログラム。箱は、欄が関数の値である配置`F`を通して読む。verifierは
    /// 値を作ったときの配置を追わないので、このLIRはverifierを通るが、verifierが確かめない実行の前提を破る。速いモード
    /// なら語へ跳ぶ。箱のタグは1なので、`code`の監査は1つの引数を待つ値のエントリを求める。
    fn applying(field: &str) -> String {
        format!(
            "\
layout C {{ Z(tobj), C(tobj) }}
layout W {{ W(obj) }}
layout F {{ F(tobj) }}
fn entry$main() -> unit {{
{field}  let c.1: obj = con C #1(v.0)
  let w.2: obj = con W #0(c.1)
  let r.3: tobj = call g(w.2)
  decref r.3
  decref w.2
  return ()
}}
fn g(borrowed w.0: obj) -> tobj {{
  unpack w.0 F #0(f.1: tobj)
  dup f.1
  let r.2: tobj = apply f.1(())
  return r.2
}}
"
        )
    }

    #[test]
    fn an_apply_of_a_box_whose_first_field_is_a_box() {
        // `code`の監査は、最初の走査する語が即値でない値を断る
        let text = applying("  let v.0: tobj = box 4611686018427387904\n");
        assert_eq!(fault(native_checked(&text)), heap(HeapError::WrongKind));
    }

    #[test]
    fn an_apply_of_a_box_whose_first_field_is_an_integer() {
        // `C(5)`: タグは引数1つのエントリを求め、語は即値なので、断るのはimageを引くことだけである。5はimageの
        // エントリではない
        let text = applying("  let v.0: tobj = box 5\n");
        assert_eq!(fault(native_checked(&text)), heap(HeapError::WrongKind));
    }

    #[test]
    fn a_wide_clause_call_to_another_wide_signature() {
        // 広い署名の節の呼び出しの所は、別の広い署名のエントリを断る。2つの操作はimageだけの語が違うので、一方の所の
        // `code`の監査は、evidenceに入れ違えたもう一方の節のクロージャを`WrongKind`で断る
        let ints = |n: usize| -> Vec<String> { (1..=n).map(|i| format!("a.{i}: int")).collect() };
        let args: Vec<String> = (1..=47).map(|i| i.to_string()).collect();
        let text = format!(
            "\
effect Wide {{ op({op}, float) -> tobj, other({other}, float) -> tobj }}
fn entry$main() -> unit {{
  let w.0: evv = prim evv_get()
  dup w.0
  dup w.0
  let c.1: ev lin = evidence Wide((), w.0) [&other$entry, &op$entry]
  let w.2: evv = prim evv_enter(w.0, c.1)
  let u.3: unit = prim evv_set(w.2)
  let r.4: tobj = clause Wide.op({args}, 2.5)
  let u.5: unit = prim evv_set(w.0)
  let u.6: unit = drop c.1
  decref r.4
  return ()
}}
internal clause Wide.op fn op$entry(e.0: cev, {op_params}, f.48: float) -> tobj {{
  decref e.0
  return ()
}}
internal clause Wide.other fn other$entry(e.0: cev, {other_params}, f.49: float) -> tobj {{
  decref e.0
  return ()
}}
",
            op = vec!["int"; 47].join(", "),
            other = vec!["int"; 48].join(", "),
            args = args.join(", "),
            op_params = ints(47).join(", "),
            other_params = ints(48).join(", "),
        );
        assert_eq!(fault(native_unverified(&text)), heap(HeapError::WrongKind));
    }

    #[test]
    fn an_apply_of_a_clause_closure() {
        // 節の関数の静的なクロージャはimageのエントリだが、そのタグは0で、どの`apply`の所も受け入れない。そのエントリは
        // 一様な署名でなく、操作の署名を持つ
        let text = "\
layout W { W(tobj) }
layout F { F(tobj) }
effect Land { land(unit) -> tobj }
fn entry$main() -> unit {
  let w.0: obj = con W #0(&land$entry)
  let r.1: tobj = call g(w.0)
  decref r.1
  decref w.0
  return ()
}
fn g(borrowed w.0: obj) -> tobj {
  unpack w.0 F #0(f.1: tobj)
  dup f.1
  let r.2: tobj = apply f.1(())
  return r.2
}
internal clause Land.land fn land$entry(e.0: cev, u.1: unit) -> tobj {
  decref e.0
  return ()
}
";
        assert_eq!(fault(native_unverified(text)), heap(HeapError::WrongKind));
    }
}
