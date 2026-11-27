//! externの失敗を`try_panic`と`try_io`に届けるテスト。実行のテストはどれもVMの検査モードとネイティブで同じプログラムを
//! 流し、標準出力と結果の文が同じことを確かめる。ふつうに終わるrunは、censusがきれいなことも確かめる。捕まえない
//! 失敗で終わるrunは、censusを取らない致命的な誤りの道で終わるので、そのテストでは、サービスが後に何も持たないことを
//! 確かめない (docs/spec/native.mdの「externの失敗」)。
#![cfg(not(miri))]
use eml_native::LoadError;
use eml_nrt::RunConfig;
use eml_test_support::{Engine, capture, execute_finished};

use crate::common::native::{agree, agree_source, machine, native, program};

/// `agree_source`と同じだが、ネイティブのrunの標準出力が`expected`であることも確かめる。ハンドラが失敗を捕まえたことを、
/// VMと同じだというだけでなく文で確かめるためである。
fn agree_prints(source: &str, expected: &str) {
    agree_source(source);
    let host = capture();
    let out = host.output();
    let program = eml_test_support::lir(source);
    let finished = execute_finished(
        &program,
        &RunConfig::default(),
        host.into_ref(),
        Engine::Native,
    );
    assert!(
        finished.is_ok(),
        "{:?}",
        finished.err().map(|e| e.to_string())
    );
    assert_eq!(out.contents(), expected);
}

/// `agree_source`と同じだが、ネイティブのrunが`expected`の誤りで終わることも確かめる。
fn agree_fails(source: &str, expected: &str) {
    agree_source(source);
    let program = eml_test_support::lir(source);
    let finished = execute_finished(
        &program,
        &RunConfig::default(),
        capture().into_ref(),
        Engine::Native,
    );
    let error = finished.map(|_| ()).expect_err("the run fails");
    assert_eq!(error.to_string(), expected);
}

/// `effects`の宣言を持つプログラムの`load`の結果。
fn load_with(effects: &str) -> Result<(), LoadError> {
    let text = format!("{effects}fn main() -> unit {{\n  return ()\n}}\n");
    let program = program(&text);
    let mut machine = machine();
    let loaded = machine.load(&program, &eml_test_support::link(&program));
    machine.close();
    loaded.map(|_| ())
}

fn refused(effect: &str) -> Result<(), LoadError> {
    Err(LoadError::Prelude {
        effect: effect.to_string(),
    })
}

#[test]
fn prelude_with_two_panic_operations_is_refused() {
    let effects =
        "effect Prelude.Panic { never panicked(obj) -> never, never again(obj) -> never }\n";
    assert_eq!(load_with(effects), refused("Prelude.Panic"));
}

#[test]
fn prelude_whose_panicked_takes_two_objects_is_refused() {
    let effects = "effect Prelude.Panic { never panicked(obj, obj) -> never }\n";
    assert_eq!(load_with(effects), refused("Prelude.Panic"));
    let effects = "\
effect Prelude.Panic { never panicked(obj) -> never }
effect Prelude.IO { never io_failed(obj, obj) -> never }
";
    assert_eq!(load_with(effects), refused("Prelude.IO"));
}

#[test]
fn prelude_whose_panicked_returns_a_value_is_refused() {
    let effects = "effect Prelude.Panic { panicked(obj) -> tobj }\n";
    assert_eq!(load_with(effects), refused("Prelude.Panic"));
    let effects = "effect Prelude.IO { io_failed(enum, obj) -> tobj }\n";
    assert_eq!(load_with(effects), refused("Prelude.IO"));
}

/// `try_io`の節は、VMと同じ`IOErrorKind`を受ける。`PermissionDenied`は、どのOSでも同じに起こせる`fail_io`で起こす。
#[test]
fn try_io_sees_the_vm_kind_for_not_found_and_permission_denied() {
    agree_prints(
        "\
import FileIn

report : Result IOError Unit -> <IO> Unit
report (Ok ()) = println \"ok\"
report (Err e) = println (show e.kind)

main : Unit -> <IO, Panic> Unit
main () =
  report (try_io (fn () -> FileIn.close (FileIn.open \"no/such/dir/missing.txt\")))
  report (try_io (fn () -> fail_io PermissionDenied \"no entry\"))
",
        "NotFound\nPermissionDenied\n",
    );
}

/// `Prelude.Panic`をエフェクトの表に持たないプログラムでは、失敗はハンドラを探さずにrunを終え、文はVMと同じである。
/// runは致命的な誤りの道で終わるので、censusは取らない。
#[test]
fn an_overflow_without_prelude_panic_ends_with_the_vm_text() {
    agree(
        "\
fn main() -> unit {
  let n.0: int = extern \"Prelude.Num Int.+\"(9223372036854775807, 1) @\"main.em\":2:3
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
",
    );
}

#[test]
fn try_panic_catches_inline_overflow() {
    agree_prints(
        "\
bump : Int -> <Panic> Int
bump n = if n > 0 then n + 1 else bump (n + 9223372036854775807)

main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> bump 9223372036854775807)))
",
        "Err \"integer overflow\"\n",
    );
}

#[test]
fn try_panic_catches_an_index_row() {
    agree_prints(
        "\
import String

main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> String.byte_at 5 \"abc\")))
",
        "Err \"string index 5 is out of range for length 3\"\n",
    );
}

#[test]
fn try_panic_catches_the_panic_row() {
    agree_prints(
        "\
check : Int -> <Panic> Int
check n = if n > 2 then panic \"boom\" else n

main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> check 5)))
  println (show (try_panic (fn () -> check 1)))
",
        "Err \"boom\"\nOk 1\n",
    );
}

#[test]
fn try_panic_catches_a_c_row_narrowing_panic() {
    agree_prints(
        "\
extern \"C\" pure symbol \"eml_fixture_add_i32\" add : Int as int -> Int as int -> <Panic> Int as int

main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> add 1 2147483648)))
  println (show (try_panic (fn () -> add 1 2)))
",
        "Err \"argument 2 of `Main.add` does not fit in C `int32_t`: 2147483648\"\nOk 3\n",
    );
}

#[test]
fn try_io_catches_a_failing_open() {
    agree_prints(
        "\
import FileIn

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> FileIn.close (FileIn.open \"no/such/dir/missing.txt\")) with
    | Ok () -> println \"opened\"
    | Err e -> println \"caught: \\{e}\"
",
        "caught: cannot open `no/such/dir/missing.txt`: not found\n",
    );
}

/// 確保量の上限で止まった行の失敗は、`try_panic`も`try_io`も捕まえない。runは致命的な誤りの道で終わるので、censusは
/// 取らない。
#[test]
fn a_stop_is_never_caught() {
    agree_fails(
        "\
import Array

main : Unit -> <IO, Panic> Unit
main () =
  let r = try_io (fn () -> try_panic (fn () -> Array.length (Array.make 9223372036854775807 0)))
  match r with
    | Ok (Ok n) -> println (show n)
    | Ok (Err e) -> println \"panic: \\{e}\"
    | Err e -> println \"io: \\{e}\"
",
        "the memory limit is exceeded\n  at test.em:5:62",
    );
}

/// 捕まえるハンドラがなければ、runはVMと同じ文で終わる。runは致命的な誤りの道で終わるので、censusは取らない。
#[test]
fn no_handler_gives_the_vm_text() {
    agree_fails(
        "\
import String

main : Unit -> <IO, Panic> Unit
main () =
  println \"before\"
  println (show (String.byte_at 5 \"abc\"))
",
        "string index 5 is out of range for length 3\n  at test.em:6:18",
    );
}

/// 失敗した行の地点の後始末は、`eml$raise`が戻った後に走る。`Faults`の行 (`byte_at`) と戻らない行 (`panic`) の
/// どちらでも、間のフレームが持つ文字列が手放されなければcensusが漏れを見つける。
#[test]
fn cleanups_run_after_raise_returns() {
    agree_prints(
        "\
import Byte
import String

at : Int -> <Panic> Int
at n =
  let s = show (n * 1000)
  let b = String.byte_at n s
  Byte.to_int b + String.byte_length s

stop : Int -> <Panic> Int
stop n =
  let s = show (n * 1000)
  let k = if n > 3 then panic \"stop \\{s}\" else n
  k + String.byte_length s

main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> at 9)))
  println (show (try_panic (fn () -> at 1)))
  println (show (try_panic (fn () -> stop 9)))
  println (show (try_panic (fn () -> stop 1)))
",
        "Err \"string index 9 is out of range for length 4\"\nOk 52\nErr \"stop 9000\"\nOk 5\n",
    );
}

/// `try_panic`を、EPの出す形 (`prompt$`と節の関数) で手で書いたもの。節は状態 (`try_panic`の2つ目の引数) を
/// handleの値にする。
const TRY_PANIC: &str = "\
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

/// `Effectful`の行の`Panic`の類の失敗 (範囲の外の`Std.Env.arg`) は`try_panic`に届く。`Std.Env`は範囲の外の位置を
/// 渡さないので、手で書いたLIRで行を直に呼ぶ。`Effectful`の行の`IO`の類の失敗は`try_io_catches_a_failing_open`と
/// `try_io_sees_the_vm_kind_for_not_found_and_permission_denied`が確かめる。`Panics`の行の`IO`の類の失敗は起こらない。
/// `Panics`の行はどれも値の上で計算するだけでHostに触れないので、`IO`の種類を持つfaultを返さないためである。
#[test]
fn an_effectful_rows_panic_fault_is_caught_by_try_panic() {
    let text = format!(
        "\
effect Prelude.Panic {{ never panicked(obj) -> never }}
fn main() -> unit {{
  let c.0: tobj = box 42
  let r.1: tobj = call try_panic(&boom, c.0)
  decref r.1
  let s.2: obj = const \"after\"
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}}
fn boom(p.0: unit) -> tobj {{
  let s.1: obj = extern \"Std.Env.arg\"(99) @\"main.em\":2:3
  return s.1
}}
{TRY_PANIC}"
    );
    agree(&text);
    assert_eq!(native(&text), ("after\n".to_string(), Ok(None)));
}
