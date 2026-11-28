//! LIR の実行: 定数、extern、直接の呼び出しと戻り (エントリでないブロックからの戻りを含む)、末尾呼び出し、誤りと位置、
//! 終わりの処理 (docs/spec/vm.md の「呼び出しと戻り」と「誤り、止めること、終わり」)。

use eml_rt::{ArithFault, Fault, HeapError};
use eml_sched::error::{RunFault, SourceLocation};
use eml_test_support::{execute, run_stats};
use eml_vm::RuntimeError;

use crate::common::{parse, run_lir, run_unverified, unverified};

#[test]
fn hello_prints_a_line() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"hello\"
  let u.1: unit = extern Prelude.println(s.0)
  return u.1
}
";
    assert_eq!(run_lir(text), ("hello\n".to_string(), Ok(())));
}

#[test]
fn a_call_returns_its_result_to_the_caller() {
    let text = "\
fn main() -> unit {
  let n.0: int = call add(40, 2)
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
fn add(a.0: int, b.1: int) -> int {
  let c.2: int = extern \"Prelude.Num Int.+\"(a.0, b.1)
  return c.2
}
";
    assert_eq!(run_lir(text), ("42\n".to_string(), Ok(())));
}

#[test]
fn an_int_wider_than_the_immediate_is_a_constant() {
    let text = "\
fn main() -> unit {
  let n.0: int = extern \"Prelude.Num Int.+\"(4611686018427387904, 1)
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
";
    assert_eq!(run_lir(text), ("4611686018427387905\n".to_string(), Ok(())));
}

#[test]
fn a_tail_call_reuses_the_frame_header() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"tail\"
  tail call say(s.0)
}
fn say(s.0: obj) -> unit {
  let u.1: unit = extern Prelude.println(s.0)
  return u.1
}
";
    assert_eq!(run_lir(text), ("tail\n".to_string(), Ok(())));
}

#[test]
fn a_division_by_zero_carries_the_position_of_the_extern() {
    let text = "\
fn main() -> unit {
  let n.0: int = extern \"Prelude.Div Int./\"(1, 0) @\"main.em\":2:3
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
";
    let (_, result) = run_lir(text);
    let error = result.unwrap_err();
    assert_eq!(
        error,
        RuntimeError::Fault {
            fault: RunFault::Extern(Fault::Arith(ArithFault::DivisionByZero)),
            function: "main".to_string(),
            at: Some(SourceLocation {
                path: "main.em".to_string(),
                line: 2,
                column: 3,
            }),
        }
    );
    assert_eq!(error.to_string(), "division by zero\n  at main.em:2:3");
}

#[test]
fn output_before_an_error_is_flushed() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"before\"
  let u.1: unit = extern Prelude.println(s.0)
  let n.2: int = extern \"Prelude.Div Int.%\"(1, 0)
  let t.3: obj = extern \"Prelude.Show Int.show\"(n.2)
  let v.4: unit = extern Prelude.println(t.3)
  return v.4
}
";
    let (out, result) = run_lir(text);
    assert_eq!(out, "before\n");
    assert_eq!(
        result.unwrap_err().to_string(),
        "division by zero in `main`"
    );
}

#[test]
fn an_entry_that_returns_an_object_does_not_leak() {
    let text = "\
fn main() -> obj {
  let s.0: obj = extern \"Prelude.Show Int.show\"(7)
  return s.0
}
";
    assert_eq!(run_lir(text), (String::new(), Ok(())));
}

/// verifier は手放さない値を拒むので、通さずに実行して検査モードのリークの検査を確かめる。
#[test]
fn a_forgotten_object_is_a_leak() {
    let text = "\
fn main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(7)
  return ()
}
";
    let (_, result) = run_unverified(text);
    let error = result.unwrap_err();
    assert_eq!(error, RuntimeError::Leak(vec![("String".to_string(), 1)]));
    assert_eq!(
        error.to_string(),
        "memory leak: objects were not freed: 1 String"
    );
    // 速いモードはリークを確かめないので、同じ IR が成功で終わる
    let (_, fast) = execute(
        &unverified(text),
        &eml_vm::RunConfig::default().with_debug_heap(false),
    );
    assert_eq!(fast.map(|_| ()), Ok(()));
}

/// EPが書く形の`try_panic`、`prompt$`とその補助の関数、それにpanicする`boom`。
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

/// verifierが拒む関数のフレームは、中断が通っても何も手放さない (docs/spec/vm.mdの「巻き戻しの表」)。中断は正しい
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
        eml_lir::verify(&parse(&text)).map_err(|error| error.to_string()),
        Err("`s.1` is still owned at the end of the function in `leaky`".to_string())
    );
    assert_eq!(
        run_unverified(&text),
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
        eml_lir::verify(&parse(&text)).map_err(|error| error.to_string()),
        Err("`s.1` is still owned at the end of the function in `leaky`".to_string())
    );
    assert_eq!(
        run_unverified(&text),
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
fn an_unused_call_result_does_not_overwrite_a_live_value() {
    let text = "\
fn main() -> unit {
  let k.0: int = extern \"Prelude.Num Int.+\"(1, 2)
  let r.1: int = call one()
  let s.2: obj = extern \"Prelude.Show Int.show\"(k.0)
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
fn one() -> int {
  return 1
}
";
    assert_eq!(run_lir(text), ("3\n".to_string(), Ok(())));
}

#[test]
fn swapped_block_arguments_arrive_swapped() {
    let text = "\
fn main() -> unit {
  let x.0: int = extern \"Prelude.Num Int.+\"(1, 0)
  let y.1: int = extern \"Prelude.Num Int.+\"(2, 0)
  jump b1(y.1, x.0)
b1(p.2: int, q.3: int):
  let d.4: int = extern \"Prelude.Num Int.-\"(p.2, q.3)
  let s.5: obj = extern \"Prelude.Show Int.show\"(d.4)
  let u.6: unit = extern Prelude.println(s.5)
  return u.6
}
";
    assert_eq!(run_lir(text), ("1\n".to_string(), Ok(())));
}

#[test]
fn a_call_resumes_at_the_next_statement_of_its_block() {
    // エントリでないブロックの2つの呼び出しから、それぞれ次の文に戻り、呼ぶ側のスロットに残る `n.0` を合流のブロック
    // まで持ち越す (docs/spec/vm.md の「呼び出しと戻り」)
    let text = "\
fn main() -> unit {
  let r.0: int = call pick(1)
  let s.1: obj = extern \"Prelude.Show Int.show\"(r.0)
  let t.2: unit = extern Prelude.println(s.1)
  return t.2
}
fn pick(n.0: int) -> int {
  switch n.0 { 1 -> b1, _ -> b2 }
b1:
  let a.1: int = call double(n.0)
  let b.2: int = call double(a.1)
  jump b3(b.2)
b2:
  jump b3(0)
b3(m.3: int):
  let c.4: int = extern \"Prelude.Num Int.+\"(m.3, n.0)
  return c.4
}
fn double(x.0: int) -> int {
  let y.1: int = extern \"Prelude.Num Int.+\"(x.0, x.0)
  return y.1
}
";
    assert_eq!(run_lir(text), ("5\n".to_string(), Ok(())));
}

#[test]
fn a_fault_outside_an_extern_has_no_position() {
    // 位置を付けるのは extern の呼び出しが起こした誤りだけである。同じ関数に位置付きの extern があっても、`decref` の
    // 誤りには付けない
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn main() -> unit {
  let n.0: int = extern \"Prelude.Num Int.+\"(1, 2) @\"main.em\":1:1
  let d.1: obj = con (,) #0(n.0, n.0)
  decref d.1
  decref d.1
  return ()
}
";
    let (_, result) = run_unverified(text);
    assert_eq!(
        result,
        Err(RuntimeError::Fault {
            fault: RunFault::Extern(Fault::Heap(HeapError::UseAfterFree)),
            function: "main".to_string(),
            at: None,
        })
    );
}

#[test]
fn a_heap_fault_inside_an_extern_with_a_position_carries_it() {
    // 位置は extern の呼び出しが起こした誤りすべてに付き、ヒープの誤りも含む (docs/spec/vm.md の「誤り、止めること、
    // 終わり」)
    let text = "\
fn main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(1)
  decref s.0
  let t.1: unit = extern Prelude.println(s.0) @\"main.em\":2:3
  return t.1
}
";
    let (_, result) = run_unverified(text);
    let error = result.unwrap_err();
    assert_eq!(
        error,
        RuntimeError::Fault {
            fault: RunFault::Extern(Fault::Heap(HeapError::UseAfterFree)),
            function: "main".to_string(),
            at: Some(SourceLocation {
                path: "main.em".to_string(),
                line: 2,
                column: 3,
            }),
        }
    );
    assert_eq!(error.to_string(), "use of a freed object\n  at main.em:2:3");
}

#[test]
fn a_dup_of_a_file_is_a_runtime_error() {
    // verifier は `lin` の変数の `dup` を V1 で静的に拒む。印を誤った IR でも、検査モードのヒープが実行時に止める
    // ことを、verifier を通さずに確かめる。速いモードはこれを見つけない (docs/spec/object-model.md の「`lin` ビット」と
    // 「受け入れる制約」)
    // 文字列のリテラルに埋めるので、Windows の `\` は `/` にする
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").replace('\\', "/");
    let text = format!(
        "\
fn main() -> unit {{
  let p.0: obj = const \"{path}\"
  let u.1: unit = call f(p.0)
  return u.1
}}
fn f(p.0: obj) -> unit {{
  let h.1: obj lin = extern Std.FileIn.open(p.0)
  dup h.1
  let u.2: unit = extern Std.FileIn.close(h.1)
  let u.3: unit = extern Std.FileIn.close(h.1)
  return u.3
}}
"
    );
    assert_eq!(
        eml_lir::verify(&parse(&text)).map_err(|error| error.to_string()),
        Err("`h.1` (lin) is duplicated, but only unr can be in `f`".to_string())
    );
    let (_, result) = run_unverified(&text);
    let error = result.unwrap_err();
    assert_eq!(
        error,
        RuntimeError::Fault {
            fault: RunFault::Extern(Fault::Heap(HeapError::LinDup)),
            function: "f".to_string(),
            at: None,
        }
    );
    assert_eq!(error.to_string(), "a Lin object is shared in `f`");
}

/// 種類が2つ以上のリークは、種類ごとの数を `, ` でつないで表示する。
#[test]
fn leaks_are_displayed_with_the_object_counts() {
    let leak = RuntimeError::Leak(vec![("Closure".to_string(), 2), ("String".to_string(), 1)]);
    assert_eq!(
        leak.to_string(),
        "memory leak: objects were not freed: 2 Closure, 1 String"
    );
}

/// 浮動小数のビット列を `Int` として表示する。verifier は値を作った配置を確かめないので
/// (`a_value_read_with_another_layout_passes_the_verifier_and_faults`)、`float` のフィールドの箱を `int` の
/// 配置で分解して、同じ生の語を読む。`Show Float` は後の作業で足す。
const FLOAT_BITS: &str = "\
layout List { Nil, Cons(tobj, tobj) }
layout Pair { Pair(tobj, tobj) }
layout F { F(float) }
layout I { I(int) }
layout F2 { F2(float, float) }
layout I2 { I2(int, int) }
";

/// `box 1.5` を1要素のリストに入れて取り出し、`unbox` して `box` し直し、もう一度 `unbox` してビット列を表示する。
#[test]
fn boxed_float_survives_a_list() {
    let text = format!(
        "{FLOAT_BITS}\
fn main() -> unit {{
  let b.0: tobj = box 1.5
  let l.1: tobj = con List #1(b.0, #0)
  switch l.1 List {{ #0 -> b1, #1(x.2: tobj, r.3: tobj) -> b2 }}
b1:
  decref l.1
  return ()
b2:
  release l.1 List #1(x.2, _)
  let f.4: float = unbox x.2
  decref x.2
  let c.5: tobj = box f.4
  let g.6: float = unbox c.5
  decref c.5
  let p.7: obj = con F #0(g.6)
  unpack p.7 I #0(n.8: int)
  decref p.7
  let s.9: obj = extern \"Prelude.Show Int.show\"(n.8)
  let o.10: unit = extern Prelude.println(s.9)
  return o.10
}}
"
    );
    assert_eq!(
        run_lir(&text),
        (format!("{}\n", 1.5f64.to_bits() as i64), Ok(()))
    );
}

/// `0.0` と `-0.0` は `==` で等しいが、ビット列が違う。定数の表はビット列で引くので、2つを1つにまとめない。箱の定数と
/// 生の定数の両方で確かめる。
#[test]
fn zero_and_negative_zero_constants_stay_distinct() {
    let text = format!(
        "{FLOAT_BITS}\
fn main() -> unit {{
  let a.0: tobj = box 0.0
  let b.1: tobj = box -0.0
  let p.2: obj = con Pair #0(a.0, b.1)
  unpack p.2 Pair #0(x.3: tobj, y.4: tobj)
  let f.5: float = unbox x.3
  let g.6: float = unbox y.4
  decref p.2
  let q.7: obj = con F2 #0(f.5, g.6)
  unpack q.7 I2 #0(m.8: int, n.9: int)
  decref q.7
  let r.10: obj = con F2 #0(0.0, -0.0)
  unpack r.10 I2 #0(j.11: int, k.12: int)
  decref r.10
  let s.13: obj = extern \"Prelude.Show Int.show\"(m.8)
  let o.14: unit = extern Prelude.println(s.13)
  let s.15: obj = extern \"Prelude.Show Int.show\"(n.9)
  let o.16: unit = extern Prelude.println(s.15)
  let s.17: obj = extern \"Prelude.Show Int.show\"(j.11)
  let o.18: unit = extern Prelude.println(s.17)
  let s.19: obj = extern \"Prelude.Show Int.show\"(k.12)
  let o.20: unit = extern Prelude.println(s.19)
  return o.20
}}
"
    );
    let min = i64::MIN;
    assert_eq!(run_lir(&text), (format!("0\n{min}\n0\n{min}\n"), Ok(())));
}

/// `thaw` は共有された配列だけを写す (docs/spec/object-model.md の「配列」)。写しは確保が 1 つ増えるので、
/// 配列を後で使うプログラムと使わないプログラムの `allocations` の差はちょうど 1 になる。
#[test]
fn thaw_copies_only_shared_arrays() {
    let unique = "import Array\nimport MutArray\nmain : Unit -> <IO, Panic> Unit
main () =
  let a = Array.make 3 0
  let n = Array.length (MutArray.freeze (Array.thaw a))
  println (show (n + 0))
";
    let shared = "import Array\nimport MutArray\nmain : Unit -> <IO, Panic> Unit
main () =
  let a = Array.make 3 0
  let n = Array.length (MutArray.freeze (Array.thaw a))
  println (show (Array.length a + n))
";
    let allocations = |text: &str| {
        let (_, result) = run_stats(text);
        result.expect("the program runs").allocations
    };
    assert_eq!(allocations(shared), allocations(unique) + 1);
}
