//! テストの記述と、C の関数として呼ぶ Rust の関数。

use eml_cabi::CCall;
use eml_extern::{CFunction, CLibrary, CLink, CParam, CRet, Os};

pub fn function(
    name: &str,
    symbol: &str,
    links: Vec<CLink>,
    params: Vec<CParam>,
    ret: CRet,
) -> CFunction {
    CFunction {
        name: name.to_owned(),
        symbol: symbol.to_owned(),
        links,
        pure: false,
        blocking: false,
        params,
        ret,
    }
}

/// 動いている OS の C の標準ライブラリ。何も入れずにどの CI の機械にもある。
pub fn c_library() -> CLink {
    let (os, name) = if cfg!(target_os = "linux") {
        (Os::Linux, "libc.so.6")
    } else if cfg!(target_os = "macos") {
        (Os::Macos, "libSystem.dylib")
    } else {
        (Os::Windows, "ucrtbase")
    };
    CLink {
        os,
        library: CLibrary::Named(name.to_owned()),
    }
}

/// 橋渡しを通して呼ぶ。
pub fn call(c: CCall, args: &[u64]) -> u64 {
    // SAFETY: テストは `args` を、呼ぶ関数の C の署名の slot の数だけ渡す
    unsafe { (c.bridge)(c.callee, args.as_ptr()) }
}

pub extern "C" fn add_i32(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

#[allow(clippy::too_many_arguments)]
pub extern "C" fn sum12(
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
    [a, c, e, f, g, h, i, j]
        .into_iter()
        .map(i64::from)
        .sum::<i64>()
        + b
        + d as i64
        + (x + y) as i64
}

pub extern "C" fn neg() -> i32 {
    -7
}

/// # Safety
/// `a` と `b` は `n` 個の `f64` を指す。
pub unsafe extern "C" fn dot(a: *const f64, b: *const f64, n: i64) -> f64 {
    let n = usize::try_from(n).unwrap();
    // SAFETY: 上の約束による
    let (a, b) = unsafe {
        (
            std::slice::from_raw_parts(a, n),
            std::slice::from_raw_parts(b, n),
        )
    };
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub extern "C" fn half(x: f64) -> f64 {
    x / 2.0
}
