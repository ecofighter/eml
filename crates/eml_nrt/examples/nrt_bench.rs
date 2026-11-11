//! エントリのstubと、2つのエントリの形の費用を測る (docs/implementation/benchmarks.md の「命令の数の測り方」)。
use std::hint::black_box;
use std::sync::Arc;

use eml_nrt::{Fatal, enter};
use eml_rt::{Buffering, Control, Ctx, Fast, HostRef, Limits, StdHost};

eml_nrt::entries! {
    const BENCH;

    plain fn eml_t_plain(ctx, a: i64) -> i64 {
        let _ = ctx;
        a + 1
    }

    shim fn eml_t_shim(ctx, a: i64) -> i64 {
        let _ = ctx;
        Ok::<i64, Fatal>(a + 1)
    }
}

unsafe extern "C" fn add_one(_ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
    unsafe { *argv += 1 };
}

fn main() {
    let case = std::env::args().nth(1).unwrap_or_default();
    let host = StdHost::new(
        Box::new(std::io::sink()),
        Box::new(std::io::sink()),
        std::path::PathBuf::new(),
        Buffering::Block,
    );
    let mut ctx = Box::new(Ctx::<Fast>::new(
        Arc::new(Control::new(Limits::default())),
        HostRef::new(Arc::new(host)),
    ));
    let raw: *mut Ctx<Fast> = &mut *ctx;
    let plain = black_box(BENCH[0].address)();
    let shim = black_box(BENCH[1].address)();
    type F = unsafe extern "C" fn(*mut Ctx<Fast>, i64) -> i64;
    let mut x = 0i64;
    match case.as_str() {
        "empty" => {}
        "enter" => {
            let mut argv = [0u64];
            for _ in 0..10_000_000 {
                unsafe {
                    enter(raw, black_box(add_one), std::ptr::null(), argv.as_mut_ptr()).unwrap()
                };
            }
            x = argv[0] as i64;
        }
        "entry-plain" => {
            let f = unsafe { std::mem::transmute::<*const u8, F>(plain) };
            for _ in 0..100_000_000 {
                x = unsafe { f(raw, x) };
            }
        }
        "entry-shim" => {
            let f = unsafe { std::mem::transmute::<*const u8, F>(shim) };
            for _ in 0..100_000_000 {
                x = unsafe { f(raw, x) };
            }
        }
        other => panic!("a case: empty, enter, entry-plain, entry-shim; got {other:?}"),
    }
    println!("{x}");
}
