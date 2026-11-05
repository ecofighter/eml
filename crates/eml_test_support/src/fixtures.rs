//! UI テストとトランスクリプトが C の関数の代わりに呼ぶ Rust の関数。アドレスで `Linker` に登録するので、C の
//! コンパイラも共有ライブラリも要らず、どの OS でも同じに動く (docs/implementation/testing.md の「UI テスト」)。

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

/// `Linker::with_symbols` に渡す、このファイルの `eml_fixture_*` のすべて。
pub fn fixture_symbols() -> Vec<(&'static str, *const ())> {
    vec![
        ("eml_fixture_add_i32", eml_fixture_add_i32 as *const ()),
        ("eml_fixture_sum12", eml_fixture_sum12 as *const ()),
        ("eml_fixture_neg", eml_fixture_neg as *const ()),
        ("eml_fixture_dot", eml_fixture_dot as *const ()),
        ("eml_fixture_fill", eml_fixture_fill as *const ()),
        ("eml_fixture_bytes_sum", eml_fixture_bytes_sum as *const ()),
        (
            "eml_fixture_handle_new",
            eml_fixture_handle_new as *const (),
        ),
        (
            "eml_fixture_handle_get",
            eml_fixture_handle_get as *const (),
        ),
        (
            "eml_fixture_handle_free",
            eml_fixture_handle_free as *const (),
        ),
        ("eml_fixture_frees", eml_fixture_frees as *const ()),
        ("eml_fixture_gate_open", eml_fixture_gate_open as *const ()),
        ("eml_fixture_gate_wait", eml_fixture_gate_wait as *const ()),
        (
            "eml_fixture_handle_wait",
            eml_fixture_handle_wait as *const (),
        ),
    ]
}

pub extern "C" fn eml_fixture_add_i32(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

/// 引数ごとに重みを変えて足すので、引数の取り違えと符号の広げ損ねが結果に出る。レジスタに乗らない引数 (スタック渡し)
/// と、整数と浮動小数点のレジスタの混ざりも通る。
#[allow(clippy::too_many_arguments)]
pub extern "C" fn eml_fixture_sum12(
    a: i32,
    b: i64,
    c: i32,
    d: usize,
    e: i32,
    f: i32,
    g: i32,
    h: i32,
    i: i32,
    j: i32,
    x: f64,
    y: f64,
) -> i64 {
    i64::from(a)
        + 2 * b
        + 3 * i64::from(c)
        + 4 * d as i64
        + 5 * i64::from(e)
        + 6 * i64::from(f)
        + 7 * i64::from(g)
        + 8 * i64::from(h)
        + 9 * i64::from(i)
        + 10 * i64::from(j)
        + (11.0 * x + 12.0 * y) as i64
}

/// 引数のない関数の結果が、符号を広げて `Int` になることを確かめる。
pub extern "C" fn eml_fixture_neg() -> i32 {
    -7
}

/// # Safety
/// `a` と `b` は `n` 個の `f64` を読める。VM が貸した配列で、呼び手が `n` を長さ以下にする。
pub unsafe extern "C" fn eml_fixture_dot(a: *const f64, b: *const f64, n: i64) -> f64 {
    // SAFETY: 上の約束による
    let (a, b) = unsafe {
        (
            std::slice::from_raw_parts(a, n as usize),
            std::slice::from_raw_parts(b, n as usize),
        )
    };
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// `p[i] = x * i` を書く。
///
/// # Safety
/// `p` は `n` 個の `f64` を書ける。VM が一意に貸した配列で、呼び手が `n` を長さ以下にする。
pub unsafe extern "C" fn eml_fixture_fill(p: *mut f64, n: i64, x: f64) {
    // SAFETY: 上の約束による
    let a = unsafe { std::slice::from_raw_parts_mut(p, n as usize) };
    for (i, y) in a.iter_mut().enumerate() {
        *y = x * i as f64;
    }
}

/// # Safety
/// `p` は `n` バイトを読める。VM が貸した文字列かバイト列で、呼び手が `n` を長さ以下にする。
pub unsafe extern "C" fn eml_fixture_bytes_sum(p: *const u8, n: i64) -> i64 {
    // SAFETY: 上の約束による
    let bytes = unsafe { std::slice::from_raw_parts(p, n as usize) };
    bytes.iter().map(|&b| i64::from(b)).sum()
}

struct Handle {
    tag: i64,
    value: i64,
}

/// `value` が負なら NULL を返す。
pub extern "C" fn eml_fixture_handle_new(tag: i64, value: i64) -> *mut c_void {
    if value < 0 {
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(Handle { tag, value })).cast()
}

/// # Safety
/// `h` は `eml_fixture_handle_new` が返し、まだ解放していない。VM は貸している間だけ使う。
pub unsafe extern "C" fn eml_fixture_handle_get(h: *mut c_void) -> i64 {
    // SAFETY: 上の約束による
    unsafe { (*h.cast::<Handle>()).value }
}

/// 解放した数を tag ごとに数える。
///
/// # Safety
/// `h` は `eml_fixture_handle_new` が返し、まだ解放していない。dtor が1回だけ呼ぶ。
pub unsafe extern "C" fn eml_fixture_handle_free(h: *mut c_void) {
    // SAFETY: 上の約束による
    let handle = unsafe { Box::from_raw(h.cast::<Handle>()) };
    *FREES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_default()
        .entry(handle.tag)
        .or_default() += 1;
}

/// tag ごとの解放の数。ファイルごとに tag が違うので、並べて走る実行の数が混ざらない。
static FREES: Mutex<Option<HashMap<i64, i64>>> = Mutex::new(None);

/// `tag` の解放の数を返して 0 に戻す。
pub extern "C" fn eml_fixture_frees(tag: i64) -> i64 {
    FREES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_default()
        .remove(&tag)
        .unwrap_or(0)
}

/// 開いた鍵の番号。プロセスで1つで、待ちが消費する。
static GATES: Mutex<Option<HashSet<i64>>> = Mutex::new(None);
static GATES_CHANGED: Condvar = Condvar::new();

/// テストが壊れて鍵が開かないときに、プロセスを止めないための見張り。ふつうの run では届かず、結果はこの時間で決まらない。
const GATE_WATCHDOG: Duration = Duration::from_secs(10);

/// 鍵 `id` を開く。開いた鍵は1回の `eml_fixture_gate_wait` が消費する。待ちが先でも後でも1回ずつ対になる。
pub extern "C" fn eml_fixture_gate_open(id: i64) {
    GATES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_default()
        .insert(id);
    GATES_CHANGED.notify_all();
}

/// 鍵 `id` が開くまで待ち、消費して 0 を返す。見張りの間に開かなければ -1 を返す。
pub extern "C" fn eml_fixture_gate_wait(id: i64) -> i64 {
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

/// 鍵 `id` を待ってから、ハンドル `h` の値 (`eml_fixture_handle_get` と同じ) を返す。見張りで開かなければ -1。
///
/// # Safety
/// `h` は `eml_fixture_handle_new` が返し、まだ解放していない。VM は貸している間だけ使う。
pub unsafe extern "C" fn eml_fixture_handle_wait(h: *mut c_void, id: i64) -> i64 {
    if eml_fixture_gate_wait(id) != 0 {
        return -1;
    }
    // SAFETY: 上の約束による
    unsafe { eml_fixture_handle_get(h) }
}
