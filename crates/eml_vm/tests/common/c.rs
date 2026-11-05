//! C の関数の呼び出しのテストの道具。Cranelift の橋渡しを使わず、手で書いた Rust の橋渡しと Rust の呼ばれる関数で
//! `Linked` を作る。Miri は JIT のコードを動かせないためである。橋渡しは署名 (`CSignature`) ごとに1つで、JIT の
//! 橋渡しと同じく slot の列だけを読む (docs/spec/vm.md の「橋渡しの関数」)。ポインタの slot は呼ばれる関数が `i64`
//! で受け、自分で位置に戻す。ハンドルは確保せず、tag と値を詰めた null でない整数にする。Miri が slot を通った位置を
//! 参照外ししないためである。

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

use eml_lir::Program;
use eml_vm::{Bridge, CCall, Dtor, Linked};

/// `program` の C の表に並びをそろえた `Linked`。関数は `symbol`、dtor は `dtor` の名前で `calls` と `dtors` から引き、
/// 見つからなければ panic する。テストは `calls` と `dtors` に、このファイルの `calls()` と `dtors()` の項だけを渡す。
pub fn linked(
    program: &Program,
    calls: &[(&str, Bridge, *const ())],
    dtors: &[(&str, Dtor)],
) -> Linked {
    let calls = program
        .c_functions
        .iter()
        .map(|function| {
            let &(_, bridge, callee) = calls
                .iter()
                .find(|(symbol, ..)| *symbol == function.symbol)
                .unwrap_or_else(|| panic!("no call for `{}`", function.symbol));
            (function.clone(), CCall { bridge, callee })
        })
        .collect();
    let dtors = program
        .c_handles
        .iter()
        .map(|handle| {
            let dtor = handle.dtor.as_ref().map(|name| {
                dtors
                    .iter()
                    .find(|(symbol, _)| symbol == name)
                    .unwrap_or_else(|| panic!("no dtor `{name}`"))
                    .1
            });
            (handle.clone(), dtor)
        })
        .collect();
    // SAFETY: `calls()` は、このファイルの Rust の関数をそれぞれの署名の橋渡しと組にし、テストのプログラムはその記号を
    // 同じ署名で宣言する。組にするのは記号で引いたその宣言で、dtor も宣言の dtor の名前で引く。`rs_handle_free` は
    // `void f(void*)` で、どの関数もプロセスの間ずっとある
    unsafe { Linked::new(calls, dtors) }
}

/// `linked` に渡す、このファイルのすべての呼ばれる関数と橋渡し。
pub fn calls() -> Vec<(&'static str, Bridge, *const ())> {
    vec![
        ("rs_abs", i64_to_i64, rs_abs as *const ()),
        ("rs_add_i32", i32_i32_to_i32, rs_add_i32 as *const ()),
        ("rs_sub_i32", i32_i32_to_i32, rs_sub_i32 as *const ()),
        ("rs_half", f64_to_f64, rs_half as *const ()),
        ("rs_dot", i64_i64_i64_to_f64, rs_dot as *const ()),
        ("rs_fill", i64_i64_f64_to_void, rs_fill as *const ()),
        ("rs_bytes_sum", i64_i64_to_i64, rs_bytes_sum as *const ()),
        ("rs_handle_new", i64_i64_to_i64, rs_handle_new as *const ()),
        ("rs_handle_get", i64_to_i64, rs_handle_get as *const ()),
        ("rs_frees", i64_to_i64, rs_frees as *const ()),
        ("rs_gate_open", i64_to_void, rs_gate_open as *const ()),
        ("rs_gate_wait", i64_to_i64, rs_gate_wait as *const ()),
        (
            "rs_handle_wait",
            i64_i64_to_i64,
            rs_handle_wait as *const (),
        ),
    ]
}

/// `linked` に渡す、このファイルのすべての dtor。
pub fn dtors() -> Vec<(&'static str, Dtor)> {
    vec![("rs_handle_free", rs_handle_free)]
}

/// slot `k` の語。
///
/// # Safety
/// `args` は `k + 1` 個以上の slot を持つ。
unsafe fn slot(args: *const u64, k: usize) -> u64 {
    unsafe { *args.add(k) }
}

/// (I64) -> I64
unsafe extern "C" fn i64_to_i64(callee: *const (), args: *const u64) -> u64 {
    // SAFETY: `calls` はこの署名の関数だけをこの橋渡しと組にし、VM は引数の slot をすべて渡す
    unsafe {
        let f = std::mem::transmute::<*const (), extern "C" fn(i64) -> i64>(callee);
        f(slot(args, 0) as i64) as u64
    }
}

/// (I64) -> void
unsafe extern "C" fn i64_to_void(callee: *const (), args: *const u64) -> u64 {
    // SAFETY: `i64_to_i64` と同じ
    unsafe {
        let f = std::mem::transmute::<*const (), extern "C" fn(i64)>(callee);
        f(slot(args, 0) as i64);
    }
    0
}

/// (I64, I64) -> I64
unsafe extern "C" fn i64_i64_to_i64(callee: *const (), args: *const u64) -> u64 {
    // SAFETY: `i64_to_i64` と同じ
    unsafe {
        let f = std::mem::transmute::<*const (), extern "C" fn(i64, i64) -> i64>(callee);
        f(slot(args, 0) as i64, slot(args, 1) as i64) as u64
    }
}

/// (I32, I32) -> I32。JIT の橋渡しと同じく slot の下の 32 bit を渡し、結果の符号を広げる。
unsafe extern "C" fn i32_i32_to_i32(callee: *const (), args: *const u64) -> u64 {
    // SAFETY: `i64_to_i64` と同じ
    unsafe {
        let f = std::mem::transmute::<*const (), extern "C" fn(i32, i32) -> i32>(callee);
        i64::from(f(slot(args, 0) as i32, slot(args, 1) as i32)) as u64
    }
}

/// (F64) -> F64
unsafe extern "C" fn f64_to_f64(callee: *const (), args: *const u64) -> u64 {
    // SAFETY: `i64_to_i64` と同じ
    unsafe {
        let f = std::mem::transmute::<*const (), extern "C" fn(f64) -> f64>(callee);
        f(f64::from_bits(slot(args, 0))).to_bits()
    }
}

/// (I64, I64, I64) -> F64
unsafe extern "C" fn i64_i64_i64_to_f64(callee: *const (), args: *const u64) -> u64 {
    // SAFETY: `i64_to_i64` と同じ
    unsafe {
        let f = std::mem::transmute::<*const (), extern "C" fn(i64, i64, i64) -> f64>(callee);
        f(
            slot(args, 0) as i64,
            slot(args, 1) as i64,
            slot(args, 2) as i64,
        )
        .to_bits()
    }
}

/// (I64, I64, F64) -> void
unsafe extern "C" fn i64_i64_f64_to_void(callee: *const (), args: *const u64) -> u64 {
    // SAFETY: `i64_to_i64` と同じ
    unsafe {
        let f = std::mem::transmute::<*const (), extern "C" fn(i64, i64, f64)>(callee);
        f(
            slot(args, 0) as i64,
            slot(args, 1) as i64,
            f64::from_bits(slot(args, 2)),
        );
    }
    0
}

extern "C" fn rs_abs(x: i64) -> i64 {
    x.abs()
}

/// `rs_add_i32` を呼んだ回数。範囲の外の引数で呼ばないことを確かめる。
pub static ADD_I32_CALLS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn rs_add_i32(a: i32, b: i32) -> i32 {
    ADD_I32_CALLS.fetch_add(1, Ordering::Relaxed);
    a.wrapping_add(b)
}

extern "C" fn rs_sub_i32(a: i32, b: i32) -> i32 {
    a.wrapping_sub(b)
}

extern "C" fn rs_half(x: f64) -> f64 {
    x / 2.0
}

extern "C" fn rs_dot(a: i64, b: i64, n: i64) -> f64 {
    // SAFETY: VM は2つの `UnboxedArray Float` の要素 0 の位置を貸し、呼び手は `n` を要素の数以下にする
    let (a, b) = unsafe {
        (
            std::slice::from_raw_parts(
                std::ptr::with_exposed_provenance::<f64>(a as usize),
                n as usize,
            ),
            std::slice::from_raw_parts(
                std::ptr::with_exposed_provenance::<f64>(b as usize),
                n as usize,
            ),
        )
    };
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

extern "C" fn rs_fill(p: i64, n: i64, x: f64) {
    // SAFETY: VM は一意な `MutUnboxedArray Float` の要素 0 の位置を貸し、呼び手は `n` を要素の数以下にする
    let a = unsafe {
        std::slice::from_raw_parts_mut(
            std::ptr::with_exposed_provenance_mut::<f64>(p as usize),
            n as usize,
        )
    };
    for (i, y) in a.iter_mut().enumerate() {
        *y = x * i as f64;
    }
}

extern "C" fn rs_bytes_sum(p: i64, n: i64) -> i64 {
    // SAFETY: VM は文字列かバイト列の先頭の位置を貸し、呼び手は `n` を長さ以下にする
    let bytes = unsafe {
        std::slice::from_raw_parts(
            std::ptr::with_exposed_provenance::<u8>(p as usize),
            n as usize,
        )
    };
    bytes.iter().map(|&b| i64::from(b)).sum()
}

/// ハンドルの整数に `tag` を詰める位置。
const TAG_SHIFT: u32 = 32;

/// `tag` と `value` を詰めたハンドル。`value` が負なら null を返す。
extern "C" fn rs_handle_new(tag: i64, value: i64) -> i64 {
    if value < 0 {
        return 0;
    }
    (tag << TAG_SHIFT) | value
}

extern "C" fn rs_handle_get(h: i64) -> i64 {
    h & ((1 << TAG_SHIFT) - 1)
}

/// tag ごとの dtor の回数。テストは互いに違う tag を使うので、並べて流しても数が混ざらない。
static FREES: Mutex<Option<HashMap<i64, i64>>> = Mutex::new(None);

unsafe extern "C" fn rs_handle_free(h: *mut c_void) {
    let tag = (h.addr() as i64) >> TAG_SHIFT;
    let mut frees = FREES.lock().unwrap_or_else(PoisonError::into_inner);
    *frees.get_or_insert_default().entry(tag).or_default() += 1;
}

/// `tag` の dtor の回数を返して 0 に戻す。
pub extern "C" fn rs_frees(tag: i64) -> i64 {
    let mut frees = FREES.lock().unwrap_or_else(PoisonError::into_inner);
    frees.get_or_insert_default().remove(&tag).unwrap_or(0)
}

/// 開いた鍵の番号。待ちが消費する。
static GATES: Mutex<Option<HashSet<i64>>> = Mutex::new(None);
static GATES_CHANGED: Condvar = Condvar::new();

/// テストが壊れて鍵が開かないときに、プロセスを止めないための見張り。ふつうの run では届かず、結果はこの時間で決まらない。
const GATE_WATCHDOG: Duration = Duration::from_secs(10);

/// 鍵 `id` を開く。開いた鍵は1回の `rs_gate_wait` が消費する。待ちが先でも後でも1回ずつ対になる。
pub extern "C" fn rs_gate_open(id: i64) {
    GATES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_default()
        .insert(id);
    GATES_CHANGED.notify_all();
}

/// 鍵 `id` が開くまで待ち、消費して 0 を返す。見張りの間に開かなければ -1 を返す。
pub extern "C" fn rs_gate_wait(id: i64) -> i64 {
    let gates = GATES.lock().unwrap_or_else(PoisonError::into_inner);
    let (mut gates, _) = GATES_CHANGED
        .wait_timeout_while(gates, GATE_WATCHDOG, |open| {
            !open.as_ref().is_some_and(|open| open.contains(&id))
        })
        .unwrap_or_else(PoisonError::into_inner);
    if gates.get_or_insert_default().remove(&id) {
        0
    } else {
        -1
    }
}

/// 鍵 `id` を待ってから、ハンドル `h` の値 (`rs_handle_get` と同じ) を返す。見張りで開かなければ -1。
extern "C" fn rs_handle_wait(h: i64, id: i64) -> i64 {
    if rs_gate_wait(id) != 0 {
        return -1;
    }
    rs_handle_get(h)
}
