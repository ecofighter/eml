//! 実行の駆動のテスト。thunkとエントリの代役はRustの`extern "C"`の関数である。`enter`を通るので、Miriでは流さない
//! (docs/implementation/testing.md の「`eml_nrt`の検証」)。
#![cfg(not(miri))]
use std::sync::Arc;

use eml_nrt::image::{
    Census, DATUM_BIG_INT, DATUM_FLOAT, DATUM_STRING, DatumRow, FnRow, Image, NO_FILE, Options,
    SiteRow, Slice, Start, Str,
};
use eml_nrt::run::{LIMIT_HEADROOM_BYTES, STACK_HARD_MARGIN, run};
use eml_nrt::worker::park_panic;
use eml_nrt::{Finished, HostRef, RunConfig, RunFault, RuntimeError, Thunk};
use eml_rt::{Buffering, Ctx, Fast, Kind, StdHost, Word, layout};

use crate::regs::{deepest_frame, stack_pointer};

fn host() -> HostRef {
    HostRef::new(Arc::new(StdHost::new(
        Box::new(std::io::sink()),
        Box::new(std::io::sink()),
        std::path::PathBuf::new(),
        Buffering::Block,
    )))
}

fn empty() -> Image {
    Image {
        functions: Slice::new(&[]),
        files: Slice::new(&[]),
        data: Slice::new(&[]),
    }
}

fn options(census: Census) -> Options {
    Options {
        census,
        ..Options::default()
    }
}

/// `thunk`を入口にして`image`を走らせる。`argv`は代役との受け渡しに使う。
fn run_with(
    image: &Image,
    thunk: Thunk,
    argv: &mut [u64],
    result_rc: bool,
    options: Options,
) -> Result<Finished, RuntimeError> {
    let start = Start {
        thunk,
        entry: std::ptr::null(),
        argv: argv.as_mut_ptr(),
        result_rc,
    };
    // SAFETY: `image`の表はこの呼び出しの間生きていて、代役のthunkは`argv`の中だけを読み書きする
    unsafe { run(image, &start, &RunConfig::default(), host(), options) }
}

fn native_fault(fault: RunFault) -> RuntimeError {
    RuntimeError::Fault {
        fault,
        function: "<native code>".to_string(),
        at: None,
    }
}

/// 数える物体を1つ作り、`argv[0]`に結果として置く。
unsafe extern "C" fn make_data(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
    unsafe {
        let w = (*ctx).heap_mut().alloc_data(0, 0, 0, false);
        argv.cast::<Word>().write(w);
    }
}

/// 結果を手放す実行は、censusで何も残さずに終わる。
#[test]
fn returns_and_finishes() {
    let mut argv = [0u64];
    let finished = run_with(&empty(), make_data, &mut argv, true, options(Census::Check)).unwrap();
    assert_eq!(finished.exit, None);
    assert_eq!(finished.workers, 0);
}

/// 自分のスタックポインタと`stack_limit`を`argv`に書く。
unsafe extern "C" fn note_stack(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
    let frame = stack_pointer();
    unsafe {
        *argv = frame as u64;
        *argv.add(1) = (*ctx).stack_limit() as u64;
    }
}

#[test]
fn the_stack_limit_is_three_mebibytes_above_the_base() {
    let mut argv = [0u64; 2];
    let options = Options::default();
    run_with(&empty(), note_stack, &mut argv, false, options).unwrap();
    let (frame, limit) = (argv[0] as usize, argv[1] as usize);
    assert!(
        limit < frame,
        "the limit {limit:#x} is not below the frame {frame:#x}"
    );
    let margins = STACK_HARD_MARGIN + LIMIT_HEADROOM_BYTES;
    assert!(
        frame - limit <= options.stack - margins,
        "the limit {limit:#x} is too far below the frame {frame:#x}"
    );
    // 予約の底より上にあるフレーム (スレッドの始めの局所変数より上と、代役までのフレーム) の分だけ近くてよい
    const SLACK: usize = 256 << 10;
    assert!(
        frame - limit >= options.stack - margins - SLACK,
        "the limit {limit:#x} is too close to the frame {frame:#x}"
    );
    assert_eq!(STACK_HARD_MARGIN + LIMIT_HEADROOM_BYTES, 3 << 20);
}

unsafe extern "C" fn panic_and_return(ctx: *mut Ctx<Fast>, _entry: *const u8, _argv: *mut u64) {
    // SAFETY: 代役は`run`の`enter`の中で呼ばれ、`ctx.runtime()`はそのrunの`Worker`を指す
    unsafe { park_panic(&mut *ctx, Word::UNIT) }
}

#[test]
fn a_parked_panic_left_at_the_end_is_reported() {
    let error = run_with(
        &empty(),
        panic_and_return,
        &mut [0],
        false,
        Options::default(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        native_fault(RunFault::Internal("a fault is left parked at the end"))
    );
}

/// `exit`の終了コードを置かずに中断を始めて戻る。
unsafe extern "C" fn abort_without_exit(ctx: *mut Ctx<Fast>, _entry: *const u8, _argv: *mut u64) {
    unsafe { (*ctx).exit_abort() }.unwrap();
}

#[test]
fn an_abort_without_exit_is_an_internal_error() {
    let error = run_with(
        &empty(),
        abort_without_exit,
        &mut [0],
        false,
        Options::default(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        native_fault(RunFault::Internal(
            "an abort reached the bottom of the task"
        ))
    );
}

#[test]
fn the_census_reports_a_counted_object() {
    let error = run_with(&empty(), make_data, &mut [0], false, options(Census::Check)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "memory leak: objects were not freed: 1 Data"
    );
}

/// 3つのslotの物体を確かめる。文字列、大きい`int`、`float`の中身と、どれも不死であることの4つを、合っていれば1として
/// `argv[0..4]`に書く。
unsafe extern "C" fn read_slots(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
    unsafe {
        let slots = argv.cast::<*const Word>().read();
        let word = |i: usize| *slots.add(i);
        let ctx = &mut *ctx;
        let (text, big, float) = (word(0), word(1), word(2));
        let boxed = |w: Word, kind: Kind, value: u64| {
            let heap = ctx.heap();
            heap.meta(w).map(|m| m.kind()) == Ok(kind)
                && heap.raw(w, layout::BOX_VALUE_WORD) == Ok(value)
        };
        let big_ok = boxed(big, Kind::BigInt, i64::MIN as u64);
        let float_ok = boxed(float, Kind::Float, (-0.0f64).to_bits());
        let text_ok = ctx.copy_string(text, usize::MAX).as_deref() == Ok("hello");
        *argv = u64::from(text_ok);
        *argv.add(1) = u64::from(big_ok);
        *argv.add(2) = u64::from(float_ok);
        *argv.add(3) = u64::from(
            [text, big, float]
                .iter()
                .all(|&w| ctx.heap().is_immortal(w)),
        );
    }
}

#[test]
fn data_slots_hold_immortal_objects() {
    let mut slots = [0u64; 3];
    let base = slots.as_mut_ptr();
    let data = [
        DatumRow {
            kind: DATUM_STRING,
            bits: 0,
            text: Str::new("hello"),
            slot: base,
        },
        DatumRow {
            kind: DATUM_BIG_INT,
            bits: i64::MIN as u64,
            text: Str::new(""),
            slot: base.wrapping_add(1),
        },
        DatumRow {
            kind: DATUM_FLOAT,
            bits: (-0.0f64).to_bits(),
            text: Str::new(""),
            slot: base.wrapping_add(2),
        },
    ];
    let image = Image {
        data: Slice::new(&data),
        ..empty()
    };
    let mut argv = [0u64; 4];
    // SAFETY: `argv`は語1つ分の大きさがあり、`base`はこのテストの`slots`を指す
    unsafe { argv.as_mut_ptr().cast::<*const Word>().write(base.cast()) };
    run_with(&image, read_slots, &mut argv, false, options(Census::Check)).unwrap();
    assert_eq!(argv, [1, 1, 1, 1]);
}

/// 32 MiBのフレームを再帰し、降りた深さを`argv[0]`に書く。
unsafe extern "C" fn recurse(_ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
    let top = stack_pointer();
    let deepest = deepest_frame((32 << 20) / 4096);
    unsafe { *argv = (top - deepest) as u64 };
}

/// 深く再帰するテストは、64 MiBのworkerのスタックを明示して流す。既定の1 GiBは、ほかのすべてのテストが予約する。
#[test]
fn the_worker_stack_is_reserved() {
    let mut argv = [0u64];
    let options = Options {
        stack: 64 << 20,
        ..Options::default()
    };
    run_with(&empty(), recurse, &mut argv, false, options).unwrap();
    assert!(argv[0] >= 32 << 20, "recursed only {} bytes deep", argv[0]);
}

mod fatal {
    use eml_nrt::worker::{At, record};
    use eml_nrt::{FATAL_RECORDED, Fatal};
    use eml_rt::{Fault, HeapError};

    use super::*;

    eml_nrt::entries! {
        #[allow(dead_code)]
        const SAMPLE;

        /// コードのアドレス`function` (0ならなし) と地点`site` (負ならなし) で、誤りを記録する。
        shim fn eml_t_record(ctx, function: i64, site: i64) -> i64 {
            let site = (site >= 0).then_some(site as u32);
            let at = (function != 0).then_some(At { function: function as usize, site });
            // SAFETY: このエントリは`run`の`enter`の中でだけ呼ぶ
            let fatal = unsafe { record(ctx, RunFault::Panicked("boom".to_string()), at) };
            Err::<i64, Fatal>(fatal)
        }

        shim fn eml_t_heap_error(ctx) -> i64 {
            let _ = ctx;
            Err::<i64, HeapError>(HeapError::StaleToken)
        }

        shim fn eml_t_panic(ctx) -> i64 {
            let _ = ctx;
            if std::hint::black_box(true) {
                panic!("an entry panicked on purpose");
            }
            Ok::<i64, Fatal>(0)
        }
    }

    unsafe extern "C" fn call_record(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        unsafe { eml_t_record(ctx, *argv as i64, *argv.add(1) as i64) };
    }

    unsafe extern "C" fn call_heap_error(ctx: *mut Ctx<Fast>, _entry: *const u8, _argv: *mut u64) {
        unsafe { eml_t_heap_error(ctx) };
    }

    unsafe extern "C" fn call_panic(ctx: *mut Ctx<Fast>, _entry: *const u8, _argv: *mut u64) {
        unsafe { eml_t_panic(ctx) };
    }

    /// 2つの関数の行の代わりのコード。中身は読まず、アドレスの範囲だけを使う。
    static CODE: [u8; 64] = [0; 64];

    fn two_functions(sites: &[SiteRow]) -> [FnRow; 2] {
        let code = CODE.as_ptr();
        [
            FnRow {
                name: Str::new("g"),
                code: code.wrapping_add(32),
                size: 32,
                sites: Slice::new(&[]),
            },
            FnRow {
                name: Str::new("f"),
                code,
                size: 16,
                sites: Slice::new(sites),
            },
        ]
    }

    fn recorded(image: &Image, function: usize, site: Option<u32>) -> RuntimeError {
        let site = site.map_or(-1i64 as u64, u64::from);
        run_with(
            image,
            call_record,
            &mut [function as u64, site],
            false,
            Options::default(),
        )
        .unwrap_err()
    }

    #[test]
    fn a_recorded_error_comes_back() {
        let files = [Str::new("dir/file.em")];
        let sites = [
            SiteRow {
                file: NO_FILE,
                line: 0,
                column: 0,
            },
            SiteRow {
                file: 0,
                line: 3,
                column: 16,
            },
        ];
        let functions = two_functions(&sites);
        let image = Image {
            functions: Slice::new(&functions),
            files: Slice::new(&files),
            ..empty()
        };
        let f = CODE.as_ptr().addr();
        assert_eq!(
            recorded(&image, f, Some(1)).to_string(),
            "panicked: boom\n  at dir/file.em:3:16"
        );
        assert_eq!(
            recorded(&image, f, Some(0)).to_string(),
            "panicked: boom in `f`"
        );
        assert_eq!(
            recorded(&image, f, None).to_string(),
            "panicked: boom in `f`"
        );
        assert_eq!(
            recorded(&image, 0, None),
            native_fault(RunFault::Panicked("boom".to_string()))
        );
        assert_eq!(FATAL_RECORDED, 3);
    }

    #[test]
    fn a_heap_error_comes_back_as_its_fault() {
        let error = run_with(
            &empty(),
            call_heap_error,
            &mut [0],
            false,
            Options::default(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            native_fault(RunFault::Extern(Fault::Heap(HeapError::StaleToken)))
        );
    }

    #[test]
    fn an_entry_panic_is_an_internal_error() {
        let error =
            run_with(&empty(), call_panic, &mut [0], false, Options::default()).unwrap_err();
        assert_eq!(
            error,
            native_fault(RunFault::Internal("a runtime entry panicked"))
        );
    }

    #[test]
    fn a_code_address_finds_its_function() {
        let functions = two_functions(&[]);
        let image = Image {
            functions: Slice::new(&functions),
            ..empty()
        };
        let base = CODE.as_ptr().addr();
        let name = |address: usize| match recorded(&image, address, None) {
            RuntimeError::Fault { function, .. } => function,
            other => panic!("{other:?}"),
        };
        assert_eq!(name(base), "f");
        assert_eq!(name(base + 7), "f");
        assert_eq!(name(base + 15), "f");
        assert_eq!(name(base + 16), "<native code>");
        assert_eq!(name(base + 32), "g");
        assert_eq!(name(base + 63), "g");
        assert_eq!(name(base + 64), "<native code>");
        assert_eq!(name(base - 1), "<native code>");
    }
}
