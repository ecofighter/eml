//! 実行系のサービスのテスト。生成したコードの代役はRustの`extern "C"`の関数で、`run`を通して呼ぶ。サービスは`run`が
//! 入ったコードからだけ呼べるためである (docs/spec/native.md の「安全点」)。`enter`を通るので、Miriでは流さない
//! (docs/implementation/testing.md の「`eml_nrt`の検証」)。
#![cfg(not(miri))]
use std::sync::Arc;

use eml_nrt::image::{ACTION_CONTINUE, FnRow, Image, Options, SiteRow, Slice, Start, Str};
use eml_nrt::{Finished, HostRef, RunConfig, RuntimeError, Thunk, eml_nrt_safepoint};
use eml_rt::{Buffering, Control, Ctx, Fast, StdHost};

fn host() -> HostRef {
    HostRef::new(Arc::new(StdHost::new(
        Box::new(std::io::sink()),
        Box::new(std::io::sink()),
        std::path::PathBuf::new(),
        Buffering::Block,
    )))
}

/// 関数`f`の行の代わりのコード。中身は読まず、アドレスの範囲だけを使う。
static CODE: [u8; 16] = [0; 16];

fn code() -> u64 {
    CODE.as_ptr().addr() as u64
}

/// `sites`を地点に持つ関数`f`だけのプログラムを、`thunk`を入口にして走らせる。
fn run_f(sites: &[SiteRow], thunk: Thunk, argv: &mut [u64]) -> Result<Finished, RuntimeError> {
    let files = [Str::new("dir/file.em")];
    let functions = [FnRow {
        name: Str::new("f"),
        code: CODE.as_ptr(),
        size: CODE.len() as u32,
        sites: Slice::new(sites),
        value: std::ptr::null_mut(),
    }];
    let image = Image {
        functions: Slice::new(&functions),
        files: Slice::new(&files),
        data: Slice::new(&[]),
        paps: Slice::new(&[]),
    };
    let start = Start {
        thunk,
        entry: std::ptr::null(),
        argv: argv.as_mut_ptr(),
        result_rc: false,
    };
    let options = Options::default();
    // SAFETY: `image`の表はこの呼び出しの間生きていて、代役のthunkは`argv`の中だけを読み書きする
    unsafe { eml_nrt::run::run(&image, &start, &RunConfig::default(), host(), options) }
}

fn control(ctx: *mut Ctx<Fast>) -> Arc<Control> {
    // SAFETY: 代役は生きている`Ctx`を受ける
    Arc::clone(unsafe { (*ctx).heap() }.control())
}

/// workerに知らせを求めてから安全点を通り、その結果と、後のworkerの語を`argv[0..2]`に書く。サービスは致命的な
/// 誤りで戻らないことがあるので、デストラクタを持つ値をまたいで呼ばない (docs/spec/native.md の「`eml_fatal`」)。
unsafe extern "C" fn notice_then_safepoint(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
    control(ctx).notice(0);
    unsafe {
        *argv = eml_nrt_safepoint(ctx, *argv as i64, 0) as u64;
        *argv.add(1) = u64::from(control(ctx).worker_bits(0));
    }
}

#[test]
fn a_notice_is_taken_and_the_run_continues() {
    let mut argv = [code(), u64::MAX];
    run_f(&[], notice_then_safepoint, &mut argv).unwrap();
    assert_eq!(argv, [ACTION_CONTINUE as u64, 0]);
}

mod fatal {
    use eml_nrt::image::{ENTRY_SITE, NO_FILE};
    use eml_nrt::worker::park_panic;
    use eml_nrt::{FATAL_RECORDED, RunFault, eml_fatal, eml_nrt_failed, enter};
    use eml_rt::{ArithFault, CTX_FAULT, Fault};

    use super::*;

    /// 位置のある地点0と、位置のない地点1。
    const SITES: [SiteRow; 2] = [
        SiteRow {
            file: 0,
            line: 3,
            column: 16,
        },
        SiteRow {
            file: NO_FILE,
            line: 0,
            column: 0,
        },
    ];

    /// 割り込みを立ててから安全点を通る。
    unsafe extern "C" fn interrupt_then_safepoint(
        ctx: *mut Ctx<Fast>,
        _entry: *const u8,
        argv: *mut u64,
    ) {
        control(ctx).interrupt();
        unsafe { eml_nrt_safepoint(ctx, *argv as i64, 0) };
    }

    #[test]
    fn a_stop_bit_is_fatal_with_the_function() {
        let error = run_f(&[], interrupt_then_safepoint, &mut [code()]).unwrap_err();
        assert_eq!(error.to_string(), "interrupted in `f`");
        assert!(error.interrupted());
    }

    /// 0で割ったfaultを置き、`argv[0]`の関数の地点`argv[1]`で失敗を上げる。
    unsafe extern "C" fn divide_by_zero(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        unsafe {
            let _ = (*ctx).set_fault(Fault::Arith(ArithFault::DivisionByZero));
            eml_nrt_failed(ctx, *argv as i64, *argv.add(1) as i64, 0);
        }
    }

    fn divided_at(site: i64) -> String {
        run_f(&SITES, divide_by_zero, &mut [code(), site as u64])
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn a_parked_fault_is_reported_at_its_site() {
        assert_eq!(divided_at(0), "division by zero\n  at dir/file.em:3:16");
    }

    /// 位置のない地点と関数の入口は、関数を名指す。
    #[test]
    fn a_site_without_a_position_names_the_function() {
        assert_eq!(divided_at(1), "division by zero in `f`");
        assert_eq!(divided_at(ENTRY_SITE), "division by zero in `f`");
    }

    fn fault_word(ctx: *mut Ctx<Fast>) -> u64 {
        // SAFETY: `CTX_FAULT`は`Ctx`の中の語の位置である
        unsafe { ctx.cast::<u8>().add(CTX_FAULT).cast::<u64>().read() }
    }

    /// 不死でない生きている物体の数。
    fn mortal_objects(ctx: *mut Ctx<Fast>) -> u64 {
        // SAFETY: 代役は生きている`Ctx`を受け、数える間、ほかに借用はない
        unsafe {
            (*ctx).reclaim();
            let heap = (*ctx).heap_mut();
            let live = heap.live_objects();
            live.into_iter().filter(|&w| !heap.is_immortal(w)).count() as u64
        }
    }

    /// panicのメッセージを置き、地点0で失敗を上げる。置いた後の生きている物体の数を`argv[4]`に書く。
    unsafe extern "C" fn panic_and_raise(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        unsafe {
            let message = (*ctx).heap_mut().alloc_string("oh no");
            park_panic(&mut *ctx, message);
            *argv.add(1) = fault_word(ctx);
            *argv.add(4) = mortal_objects(ctx);
            eml_nrt_failed(ctx, *argv as i64, 0, 0);
        }
    }

    /// 入れ子の`enter`の中でpanicを上げ、戻った後のfaultの語を`argv[2]`に、生きている物体の数を`argv[5]`に書いてから、
    /// 同じ符号で投げ直す。サービスがメッセージの語をちょうど1回手放したことを、物体の数で確かめる。
    unsafe extern "C" fn raise_inside(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        unsafe {
            let fatal = enter(ctx, panic_and_raise, std::ptr::null(), argv).unwrap_err();
            *argv.add(2) = fault_word(ctx);
            *argv.add(3) = u64::from(fatal.code);
            *argv.add(5) = mortal_objects(ctx);
            eml_fatal(ctx, i64::from(fatal.code), i64::from(fatal.site));
        }
    }

    #[test]
    fn a_parked_panic_reports_its_text() {
        let mut argv = [code(), 0, u64::MAX, 0, 0, u64::MAX];
        let error = run_f(&SITES, raise_inside, &mut argv).unwrap_err();
        assert_eq!(error.to_string(), "panicked: oh no\n  at dir/file.em:3:16");
        assert_ne!(argv[1], 0, "the fault word before the service");
        assert_eq!(argv[2], 0, "the fault word after the service");
        assert_eq!(argv[3], u64::from(FATAL_RECORDED));
        assert_eq!(
            (argv[4], argv[5]),
            (1, 0),
            "the live objects before and after"
        );
    }

    /// 何も置かずに失敗を上げる。
    unsafe extern "C" fn raise_nothing(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        unsafe { eml_nrt_failed(ctx, *argv as i64, 0, 0) };
    }

    #[test]
    fn a_raise_with_nothing_parked_is_an_internal_error() {
        let error = run_f(&SITES, raise_nothing, &mut [code()]).unwrap_err();
        assert!(matches!(
            &error,
            RuntimeError::Fault {
                fault: RunFault::Internal("a failure was raised with nothing parked"),
                function,
                at: Some(_),
            } if function == "f"
        ));
        assert!(error.is_broken());
    }
}
