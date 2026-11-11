//! エントリのstubのテスト。ホストのABIのstubを使うので、Miriでは流さない (docs/implementation/testing.md の「`eml_nrt`の検証」)。
#![cfg(not(miri))]
use std::sync::Arc;

use eml_nrt::{FATAL_PANICKED, Fatal, JmpBuf, detached, eml_fatal, enter};
use eml_rt::{Buffering, Control, Ctx, Fast, HostRef, Limits, StdHost};

use crate::regs::{
    clobber_thunk, deepest_frame, enter_noting_fp, enter_with_patterns, fp_thunk, stack_pointer,
};

fn ctx() -> Box<Ctx<Fast>> {
    let host = StdHost::new(
        Box::new(std::io::sink()),
        Box::new(std::io::sink()),
        std::path::PathBuf::new(),
        Buffering::Block,
    );
    let control = Arc::new(Control::new(Limits::default()));
    Box::new(Ctx::new(control, HostRef::new(Arc::new(host))))
}

unsafe extern "C" fn add_one(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
    unsafe {
        assert!(!(*ctx).jmp().is_null());
        *argv += 1;
    }
}

#[test]
fn a_thunk_returns_through_enter() {
    let mut ctx = ctx();
    let mut argv = [41u64];
    let r = unsafe { enter(&mut *ctx, add_one, std::ptr::null(), argv.as_mut_ptr()) };
    assert_eq!(r, Ok(()));
    assert_eq!(argv[0], 42);
    assert!(detached(&ctx));
}

/// `fp_thunk`が渡すフレームポインタから記録をたどり、`argv[1]`の値 (`eml_enter`を呼んだときのフレームポインタ) に
/// 3つ以内で着けば`argv[0]`に1を書く。
pub unsafe extern "C" fn walk_frames(
    _ctx: *mut Ctx<Fast>,
    _entry: *const u8,
    argv: *mut u64,
    fp: *const u64,
) {
    unsafe {
        let wanted = *argv.add(1);
        let mut record = fp;
        for _ in 0..3 {
            let previous = *record;
            if previous == wanted {
                *argv = 1;
                return;
            }
            record = previous as *const u64;
        }
    }
}

/// stubはフレームの記録をつなぐ。thunkが見るフレームポインタから記録をたどると、`eml_enter`を呼んだときの
/// フレームポインタに着く。
#[test]
fn the_stub_keeps_the_frame_record_chain() {
    let mut ctx = ctx();
    let mut argv = [0u64, 0];
    let before: *mut u64 = &mut argv[1];
    let status = unsafe {
        enter_noting_fp(
            &mut *ctx,
            fp_thunk,
            std::ptr::null(),
            argv.as_mut_ptr(),
            before,
        )
    };
    assert_eq!((status, argv[0]), (0, 1));
    assert!(detached(&ctx));
}

/// ネイティブの実行系のworkerは、1 GiBを予約したスタックで動く。その予約で作ったスレッドが、既定のスレッドの
/// スタックより深く再帰して戻れることを、ネイティブのコードがこの配置に頼る前に、このファイルを組む対象で確かめる。
#[test]
fn a_thread_with_the_worker_stack_recurses() {
    const RESERVED: usize = 1 << 30;
    const DEPTH: usize = 16 << 20;
    let span = std::thread::Builder::new()
        .stack_size(RESERVED)
        .spawn(|| {
            let top = stack_pointer();
            top - deepest_frame(DEPTH / 4096)
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(span >= DEPTH, "recursed only {span} bytes deep");
}

mod fatal {
    use super::*;

    #[inline(never)]
    unsafe extern "C" fn dive(ctx: *mut Ctx<Fast>, n: u64) -> u64 {
        if n == 0 {
            unsafe { eml_fatal(ctx, 7, 9) }
        }
        unsafe { dive(ctx, n - 1) + std::hint::black_box(1) }
    }

    unsafe extern "C" fn dive_thunk(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        unsafe {
            *argv = dive(ctx, *argv);
        }
    }

    #[test]
    fn a_fatal_from_a_deep_frame_returns_from_enter() {
        let mut ctx = ctx();
        for depth in [0u64, 1, 100] {
            let mut argv = [depth];
            let r = unsafe { enter(&mut *ctx, dive_thunk, std::ptr::null(), argv.as_mut_ptr()) };
            assert_eq!(r, Err(Fatal { code: 7, site: 9 }));
            assert_eq!(argv[0], depth);
            assert!(detached(&ctx));
        }
        let mut argv = [1u64];
        assert_eq!(
            unsafe { enter(&mut *ctx, add_one, std::ptr::null(), argv.as_mut_ptr()) },
            Ok(())
        );
    }

    unsafe extern "C" fn nest(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        unsafe {
            let outer = (*ctx).jmp().cast::<JmpBuf>();
            let mut inner = [3u64];
            let r = enter(ctx, dive_thunk, std::ptr::null(), inner.as_mut_ptr());
            assert_eq!(r, Err(Fatal { code: 7, site: 9 }));
            assert_eq!((*ctx).jmp().cast::<JmpBuf>(), outer);
            let mut inner = [10u64];
            let r = enter(ctx, link_thunk, outer.cast(), inner.as_mut_ptr());
            assert_eq!(r, Ok(()));
            *argv = inner[0];
            if *argv.add(1) != 0 {
                eml_fatal(ctx, 5, 6)
            }
        }
    }

    unsafe extern "C" fn link_thunk(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64) {
        unsafe {
            let me = (*ctx).jmp().cast::<JmpBuf>();
            assert_eq!((*me).prev.cast::<u8>().cast_const(), entry);
            *argv += 1;
        }
    }

    #[test]
    fn enters_nest_and_unlink() {
        let mut ctx = ctx();
        let mut argv = [0u64, 0];
        assert_eq!(
            unsafe { enter(&mut *ctx, nest, std::ptr::null(), argv.as_mut_ptr()) },
            Ok(())
        );
        assert_eq!(argv[0], 11);
        assert!(detached(&ctx));
        let mut argv = [0u64, 1];
        assert_eq!(
            unsafe { enter(&mut *ctx, nest, std::ptr::null(), argv.as_mut_ptr()) },
            Err(Fatal { code: 5, site: 6 })
        );
        assert!(detached(&ctx));
    }

    #[test]
    fn callee_saved_registers_survive_a_fatal() {
        let mut ctx = ctx();
        let mut argv = [0u64];
        let mut out = 0u64;
        let mask = unsafe {
            enter_with_patterns(
                &mut *ctx,
                clobber_thunk,
                std::ptr::null(),
                argv.as_mut_ptr(),
                &mut out,
            )
        };
        assert_eq!(mask, 0, "registers differ after a fatal: {mask:#x}");
        assert_eq!(out, (7 << 32) | 9);
        assert!(detached(&ctx));
        let mask = unsafe {
            enter_with_patterns(
                &mut *ctx,
                add_one,
                std::ptr::null(),
                argv.as_mut_ptr(),
                &mut out,
            )
        };
        assert_eq!((mask, out, argv[0]), (0, 0, 1));
    }

    eml_nrt::entries! {
        const SHIMS;

        shim fn eml_t_checked(ctx, x: i64) -> i64 {
            let _ = ctx.stack_limit();
            if x == 1 {
                return Err(Fatal { code: 21, site: 4 });
            }
            if x == 2 {
                panic!("a shim panics");
            }
            Ok(x * 2)
        }
    }

    unsafe extern "C" fn shim_thunk(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        unsafe {
            *argv = eml_t_checked(ctx, *argv as i64) as u64;
        }
    }

    #[test]
    fn a_shim_cuts_to_enter_on_an_error_and_on_a_panic() {
        assert_eq!(SHIMS[0].symbol, "eml_t_checked");
        let mut ctx = ctx();
        let mut argv = [5u64];
        assert_eq!(
            unsafe { enter(&mut *ctx, shim_thunk, std::ptr::null(), argv.as_mut_ptr()) },
            Ok(())
        );
        assert_eq!(argv[0], 10);
        let mut argv = [1u64];
        assert_eq!(
            unsafe { enter(&mut *ctx, shim_thunk, std::ptr::null(), argv.as_mut_ptr()) },
            Err(Fatal { code: 21, site: 4 })
        );
        let mut argv = [2u64];
        assert_eq!(
            unsafe { enter(&mut *ctx, shim_thunk, std::ptr::null(), argv.as_mut_ptr()) },
            Err(Fatal {
                code: FATAL_PANICKED,
                site: 0
            })
        );
        assert!(detached(&ctx));
        let mut argv = [5u64];
        assert_eq!(
            unsafe { enter(&mut *ctx, shim_thunk, std::ptr::null(), argv.as_mut_ptr()) },
            Ok(())
        );
    }

    eml_nrt::entries! {
        const HEAP_SHIMS;

        shim fn eml_t_heap(ctx, x: i64) -> i64 {
            let _ = ctx.stack_limit();
            if x == 0 {
                return Err(eml_rt::HeapError::StaleToken);
            }
            Ok(x)
        }
    }

    unsafe extern "C" fn heap_thunk(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        unsafe {
            *argv = eml_t_heap(ctx, *argv as i64) as u64;
        }
    }

    #[test]
    fn a_heap_error_is_parked_before_the_cut() {
        assert_eq!(HEAP_SHIMS.len(), 1);
        let mut ctx = ctx();
        let mut argv = [0u64];
        assert_eq!(
            unsafe { enter(&mut *ctx, heap_thunk, std::ptr::null(), argv.as_mut_ptr()) },
            Err(Fatal {
                code: eml_nrt::FATAL_FAULT,
                site: 0
            })
        );
        assert_eq!(
            ctx.take_fault(),
            Some(eml_rt::Fault::Heap(eml_rt::HeapError::StaleToken))
        );
        assert!(detached(&ctx));
    }
}
