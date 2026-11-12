//! エントリのマクロと記述子の行 (docs/spec/native.md の「記述子の表」、「包みの本体」)。
#![allow(unsafe_code)]

/// エントリの引数と結果の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    I64,
    F64,
}

/// エントリの記述子の行。生成したコードは記号で取り込み、呼び出しの型をこの行だけから作る。
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub symbol: &'static str,
    pub address: fn() -> *const u8,
    /// `ctx`の後の引数。
    pub params: &'static [Kind],
    pub result: Option<Kind>,
}

#[doc(hidden)]
#[macro_export]
macro_rules! __kind {
    (i64) => {
        $crate::Kind::I64
    };
    (f64) => {
        $crate::Kind::F64
    };
    (obj) => {
        $crate::Kind::I64
    };
    (tobj) => {
        $crate::Kind::I64
    };
    (token) => {
        $crate::Kind::I64
    };
    (unit) => {
        $crate::Kind::I64
    };
    (int) => {
        $crate::Kind::I64
    };
    (enum) => {
        $crate::Kind::I64
    };
    (float) => {
        $crate::Kind::F64
    };
}

/// 引数と結果のRustの型。`i64`と`f64`のほかに、行の表が書くReprの名前も受ける。ほかのトークンはコンパイルの誤りになる。
/// 機械の型はどの引数も`i64`か`f64`だが、物体の語 (`obj`、`tobj`、`token`) のRustの型は`Word`である。`Word`は
/// `repr(transparent)`のポインタなので、どの対象でも`i64`と同じ渡し方で、語のprovenanceが入口をまたいで残る
/// (docs/spec/native.md の「エントリのABI」)。
#[doc(hidden)]
#[macro_export]
macro_rules! __ty {
    (!) => {
        !
    };
    (never) => {
        ()
    };
    (f64) => {
        f64
    };
    (float) => {
        f64
    };
    (i64) => {
        i64
    };
    (obj) => {
        $crate::__rt::Word
    };
    (tobj) => {
        $crate::__rt::Word
    };
    (token) => {
        $crate::__rt::Word
    };
    (unit) => {
        i64
    };
    (int) => {
        i64
    };
    (enum) => {
        i64
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __result {
    () => {
        None
    };
    (!) => {
        None
    };
    (never) => {
        None
    };
    ($t:tt) => {
        Some($crate::__kind!($t))
    };
}

/// 1つのエントリの関数。`shim`の本体の規則はここにだけ書く (docs/spec/native.md の「包みの本体」)。
#[doc(hidden)]
#[macro_export]
macro_rules! __entry {
    (plain [$(#[$m:meta])*] $v:vis $name:ident $ctx:ident ($($p:ident : $t:tt),*) ($($r:tt)?) $body:block) => {
        $(#[$m])*
        #[allow(clippy::missing_safety_doc)]
        $v unsafe extern "C" fn $name(
            $ctx: *mut $crate::__rt::Ctx<$crate::__rt::Fast>
            $(, $p: $crate::__ty!($t))*
        ) $(-> $crate::__ty!($r))? $body
    };
    (shim [$(#[$m:meta])*] $v:vis $name:ident $ctx:ident ($($p:ident : $t:tt),*) ($($r:tt)?) $body:block) => {
        $(#[$m])*
        #[allow(clippy::missing_safety_doc)]
        $v unsafe extern "C" fn $name(
            ctx: *mut $crate::__rt::Ctx<$crate::__rt::Fast>
            $(, $p: $crate::__ty!($t))*
        ) $(-> $crate::__ty!($r))? {
            let outcome = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                // SAFETY: 呼ぶ側は、このworkerの生きている`Ctx`を渡す。この借用は閉包の中で終わる
                let $ctx: &mut $crate::__rt::Ctx<$crate::__rt::Fast> = unsafe { &mut *ctx };
                $body
            }));
            match outcome {
                Ok(Ok(value)) => value,
                Ok(Err(error)) => {
                    // SAFETY: 上と同じ。借用は`park`の間だけである
                    let fatal = $crate::Raise::park(error, unsafe { &mut *ctx });
                    // SAFETY: この枠は落とすものを持たない (docs/spec/native.md の「`eml_fatal`」)
                    unsafe { $crate::eml_fatal(ctx, fatal.code as i64, fatal.site as i64) }
                }
                Err(payload) => {
                    drop(payload);
                    // SAFETY: 上と同じ
                    unsafe { $crate::eml_fatal(ctx, $crate::FATAL_PANICKED as i64, 0) }
                }
            }
        }
    };
}

/// エントリの関数と記述子の表を一緒に宣言する。宣言が1つなので、関数と、生成したコードが作る呼び出しの型が食い違わない
/// (docs/spec/native.md の「記述子の表」)。結果の`!`は`plain`の形だけが使う。`shim`の本体は`Result<R, E>`を返し、
/// 安定版のRustでは`R`に`!`を書けないためである。
#[macro_export]
macro_rules! entries {
    (
        $(#[$tm:meta])* $tv:vis const $table:ident;
        $(
            $(#[$m:meta])*
            $v:vis $form:ident fn $name:ident($ctx:ident $(, $p:ident : $t:tt)* $(,)?) $(-> $r:tt)? $body:block
        )*
    ) => {
        $(
            $crate::__entry! { $form [$(#[$m])*] $v $name $ctx ($($p: $t),*) ($($r)?) $body }
        )*
        $(#[$tm])*
        $tv const $table: &[$crate::Entry] = &[
            $(
                $crate::Entry {
                    symbol: stringify!($name),
                    address: || $name as *const u8,
                    params: &[$($crate::__kind!($t)),*],
                    result: $crate::__result!($($r)?),
                },
            )*
        ];
    };
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eml_rt::{
        ArithFault, Buffering, Control, Ctx, Fast, Fault, HostRef, Kind as ObjectKind, Limits,
        StdHost, Word, layout,
    };

    use crate::{Fatal, Kind};

    crate::entries! {
        const SAMPLE;

        plain fn eml_t_plain(ctx, a: i64, b: f64) -> f64 {
            // SAFETY: テストは生きている`Ctx`を渡す
            let last = unsafe { (*ctx).runtime() }.addr();
            a as f64 + b + last as f64
        }

        shim fn eml_t_shim(ctx, a: i64) -> i64 {
            ctx.set_runtime(std::ptr::without_provenance_mut(a as usize));
            Ok::<i64, Fatal>(a + 1)
        }

        #[allow(non_snake_case)]
        shim fn eml_t_Named(ctx, a: obj, b: float, c: enum) -> int {
            ctx.set_runtime(std::ptr::without_provenance_mut(a.bits()));
            Ok::<i64, Fatal>(a.bits() as i64 + b as i64 + c)
        }

        #[allow(non_snake_case)]
        shim fn eml_t_Never(ctx, a: tobj) -> never {
            ctx.set_runtime(std::ptr::without_provenance_mut(a.bits()));
            Ok::<(), Fatal>(())
        }

        shim fn eml_t_unit(ctx, a: i64) {
            ctx.set_runtime(std::ptr::without_provenance_mut(a as usize));
            Ok::<(), Fatal>(())
        }
    }

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

    #[test]
    fn entries_run_and_describe_themselves() {
        let mut ctx = ctx();
        let raw: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: `raw`はこのテストが持つ`Ctx`で、ほかに借用はない
        unsafe {
            assert_eq!(eml_t_shim(raw, 4), 5);
            assert_eq!(eml_t_Named(raw, Word::from_bits(1), 2.0, 3), 6);
            eml_t_Never(raw, Word::from_bits(9));
            eml_t_unit(raw, 6);
            assert_eq!(eml_t_plain(raw, 1, 0.5), 7.5);
        }
        ctx.set_runtime(std::ptr::null_mut());
        let rows: Vec<_> = SAMPLE
            .iter()
            .map(|e| (e.symbol, e.params, e.result))
            .collect();
        assert_eq!(
            rows,
            [
                ("eml_t_plain", &[Kind::I64, Kind::F64][..], Some(Kind::F64)),
                ("eml_t_shim", &[Kind::I64][..], Some(Kind::I64)),
                (
                    "eml_t_Named",
                    &[Kind::I64, Kind::F64, Kind::I64][..],
                    Some(Kind::I64)
                ),
                ("eml_t_Never", &[Kind::I64][..], None),
                ("eml_t_unit", &[Kind::I64][..], None),
            ]
        );
    }

    fn live(ctx: &mut Ctx<Fast>) -> Vec<(String, u64)> {
        crate::census::census(ctx)
    }

    #[test]
    fn heap_alloc_data_gives_a_data_object() {
        use crate::heap::{eml_rt_alloc_data, eml_rt_decref};
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: `c`はこのテストが持つ`Ctx`で、`c`の最初と最後の使用の間は`c`だけを通す。物体の参照はただ1つで、
        // 子は`()`のままである
        unsafe {
            let w = eml_rt_alloc_data(c, 3, 2, 1, 0);
            let meta = (*c).heap().meta(w).unwrap();
            assert_eq!(
                (meta.kind(), meta.tag(), meta.scan(), meta.lin()),
                (ObjectKind::Data, 3, 2, false)
            );
            assert_eq!(live(&mut *c), [("Data".to_string(), 1)]);
            eml_rt_decref(c, w);
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn heap_alloc_int_boxes_a_value_that_does_not_fit() {
        use crate::heap::{eml_rt_alloc_int, eml_rt_decref};
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ
        unsafe {
            let (small, big) = (eml_rt_alloc_int(c, 5), eml_rt_alloc_int(c, i64::MIN));
            assert_eq!(small, Word::imm(5));
            let heap = (*c).heap();
            assert_eq!(heap.meta(big).unwrap().kind(), ObjectKind::BigInt);
            assert_eq!(heap.raw(big, layout::BOX_VALUE_WORD), Ok(i64::MIN as u64));
            eml_rt_decref(c, big);
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn heap_alloc_float_boxes_the_bits() {
        use crate::heap::{eml_rt_alloc_float, eml_rt_decref};
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ
        unsafe {
            let w = eml_rt_alloc_float(c, -2.5);
            let heap = (*c).heap();
            assert_eq!(heap.meta(w).unwrap().kind(), ObjectKind::Float);
            assert_eq!(heap.raw(w, layout::BOX_VALUE_WORD), Ok((-2.5f64).to_bits()));
            eml_rt_decref(c, w);
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn heap_dup_slow_adds_a_reference() {
        use crate::heap::{eml_rt_alloc_data, eml_rt_decref, eml_rt_dup_slow, eml_rt_is_last};
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ。数えた参照の数だけ手放す
        unsafe {
            let w = eml_rt_alloc_data(c, 0, 0, 1, 0);
            eml_rt_dup_slow(c, w);
            assert_eq!(eml_rt_is_last(c, w), 0);
            eml_rt_decref(c, w);
            assert_eq!(eml_rt_is_last(c, w), 1);
            eml_rt_decref(c, w);
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn heap_decref_frees_the_last_reference_and_its_children() {
        use crate::heap::{eml_rt_alloc_data, eml_rt_decref};
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ。子の参照は親の欄へ移る
        unsafe {
            let outer = eml_rt_alloc_data(c, 0, 1, 0, 0);
            let inner = eml_rt_alloc_data(c, 1, 0, 0, 0);
            (*c).heap_mut().set_field(outer, 0, inner).unwrap();
            assert_eq!(live(&mut *c), [("Data".to_string(), 2)]);
            eml_rt_decref(c, outer);
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn heap_is_last_tells_a_unique_reference() {
        use crate::heap::{eml_rt_alloc_data, eml_rt_decref, eml_rt_dup_slow, eml_rt_is_last};
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ
        unsafe {
            let w = eml_rt_alloc_data(c, 0, 0, 1, 0);
            assert_eq!(eml_rt_is_last(c, w), 1);
            eml_rt_dup_slow(c, w);
            assert_eq!(eml_rt_is_last(c, w), 0);
            eml_rt_decref(c, w);
            eml_rt_decref(c, w);
            assert_eq!(eml_rt_is_last(c, Word::imm(7)), 0);
        }
    }

    #[test]
    fn heap_free_block_frees_a_block_whose_children_are_done() {
        use crate::heap::{eml_rt_alloc_data, eml_rt_free_block};
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ。この物体は子を持たない
        unsafe {
            let w = eml_rt_alloc_data(c, 0, 0, 2, 0);
            eml_rt_free_block(c, w);
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn heap_into_token_keeps_the_address() {
        use crate::heap::{eml_rt_alloc_data, eml_rt_drop_token, eml_rt_into_token};
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ。参照はただ1つで、子はない
        unsafe {
            let w = eml_rt_alloc_data(c, 0, 0, 2, 0);
            let token = eml_rt_into_token(c, w);
            assert_eq!(token, w);
            eml_rt_drop_token(c, token);
        }
    }

    #[test]
    fn heap_drop_token_frees_an_unused_block() {
        use crate::heap::{eml_rt_alloc_data, eml_rt_drop_token, eml_rt_into_token};
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ
        unsafe {
            let token = eml_rt_into_token(c, eml_rt_alloc_data(c, 0, 0, 2, 0));
            eml_rt_drop_token(c, token);
            assert_eq!(live(&mut *c), []);
            eml_rt_drop_token(c, Word::EMPTY);
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn heap_reuse_in_place_keeps_the_block() {
        use crate::heap::{
            eml_rt_alloc_data, eml_rt_decref, eml_rt_into_token, eml_rt_reuse_in_place,
        };
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ。作り直す物体はtagとscanと`lin`が箱と同じである
        unsafe {
            let w = eml_rt_alloc_data(c, 4, 0, 1, 0);
            let token = eml_rt_into_token(c, w);
            let again = eml_rt_reuse_in_place(c, token, 4, 0, 0);
            assert_eq!(again, w);
            let meta = (*c).heap().meta(again).unwrap();
            assert_eq!(
                (meta.kind(), meta.tag(), meta.scan()),
                (ObjectKind::Data, 4, 0)
            );
            eml_rt_decref(c, again);
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn heap_con_at_rewrites_the_header() {
        use crate::heap::{eml_rt_alloc_data, eml_rt_con_at, eml_rt_decref, eml_rt_into_token};
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ。走査する語を持たないので、書く前に読まれても子をたどらない
        unsafe {
            let w = eml_rt_alloc_data(c, 4, 0, 2, 0);
            let token = eml_rt_into_token(c, w);
            let again = eml_rt_con_at(c, token, 9, 0, 2, 0);
            let meta = (*c).heap().meta(again).unwrap();
            assert_eq!(
                (meta.kind(), meta.tag(), meta.scan(), meta.lin()),
                (ObjectKind::Data, 9, 0, false)
            );
            eml_rt_decref(c, again);
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn heap_con_at_allocates_for_an_empty_token() {
        use crate::heap::{eml_rt_con_at, eml_rt_decref};
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ。空のtokenは確保になり、走査する語を持たないので読まれても子をたどらない
        unsafe {
            let w = eml_rt_con_at(c, Word::EMPTY, 6, 0, 1, 1);
            let meta = (*c).heap().meta(w).unwrap();
            assert_eq!(
                (meta.kind(), meta.tag(), meta.scan(), meta.lin()),
                (ObjectKind::Data, 6, 0, true)
            );
            (*c).heap_mut().set_lin(w, false).unwrap();
            eml_rt_decref(c, w);
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn heap_fault_arith_parks() {
        use crate::heap::eml_rt_fault_arith;
        use crate::image::{ARITH_DIVISION_BY_ZERO, ARITH_INTEGER_OVERFLOW};
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        for (kind, fault) in [
            (ARITH_DIVISION_BY_ZERO, ArithFault::DivisionByZero),
            (ARITH_INTEGER_OVERFLOW, ArithFault::IntegerOverflow),
        ] {
            // SAFETY: 上と同じ
            unsafe {
                eml_rt_fault_arith(c, kind);
                assert_eq!((*c).take_fault(), Some(Fault::Arith(fault)));
                assert_eq!((*c).take_fault(), None);
            }
        }
    }

    /// 書けない出力。
    struct Broken;

    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }

    /// `run`がエントリの間に置くのと同じ、関数のないworkerの構造体を`ctx.runtime`の後ろに置いて`f`を呼ぶ。workerは`f`の
    /// 間ずっと生きていて、置いている間は`f`に渡すポインタからだけ触れるので、workerの構造体を使うエントリの前提を満たす
    /// (docs/spec/native.md の「実行の始めと終わり」)。`enter`を通らないのでMiriでも流せる。
    fn with_worker<R>(ctx: *mut Ctx<Fast>, f: impl FnOnce(*mut crate::worker::Worker) -> R) -> R {
        use crate::image::{Image, Slice};
        let image = Image {
            functions: Slice::new(&[]),
            files: Slice::new(&[]),
            data: Slice::new(&[]),
        };
        // SAFETY: `image`の表は空で、workerより長く生きる
        let mut worker = unsafe { crate::worker::Worker::new(&image) };
        let raw: *mut crate::worker::Worker = &raw mut worker;
        // SAFETY: テストは生きている`Ctx`を渡す
        unsafe { (*ctx).set_runtime(raw.cast()) };
        let r = f(raw);
        // SAFETY: 上と同じ
        unsafe { (*ctx).set_runtime(std::ptr::null_mut()) };
        r
    }

    #[test]
    fn every_row_entry_is_named_after_its_variant() {
        use eml_sched::rows::{LISTED, Section};
        let mut expected: Vec<String> = LISTED
            .iter()
            .filter(|l| !l.inline && matches!(l.section, Section::Generic | Section::Dedicated))
            .map(|l| format!("eml_x_{:?}", l.row))
            .collect();
        expected.extend(["eml_x_Exit".to_string(), "eml_x_Panic".to_string()]);
        let symbols: Vec<&str> = crate::rows::ROWS.iter().map(|e| e.symbol).collect();
        assert_eq!(symbols, expected);
        for l in LISTED.iter().filter(|l| l.entry.is_some()) {
            assert_eq!(l.entry, Some(format!("eml_x_{:?}", l.row).as_str()));
        }
    }

    #[test]
    fn x_println_parks_a_write_failure() {
        use crate::rows::eml_x_Println;
        let host = StdHost::new(
            Box::new(Broken),
            Box::new(std::io::sink()),
            std::path::PathBuf::new(),
            Buffering::Line,
        );
        let control = Arc::new(Control::new(Limits::default()));
        let mut ctx = Box::new(Ctx::<Fast>::new(control, HostRef::new(Arc::new(host))));
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ。行は文字列の参照を受け取り、書けなくても手放す
        unsafe {
            let line = (*c).fresh_string("hello");
            assert_eq!(eml_x_Println(c, line), Word::UNIT.bits() as i64);
            let fault = c.cast::<u8>().add(eml_rt::CTX_FAULT).cast::<u64>().read();
            assert_ne!(fault, 0);
            assert!(matches!((*c).take_fault(), Some(Fault::Output(_))));
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn x_str_concat_returns() {
        use crate::rows::eml_x_StrConcat;
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ。行は両辺の参照を受け取り、結果の参照を返す
        unsafe {
            let (a, b) = ((*c).fresh_string("ab"), (*c).fresh_string("cd"));
            let joined = eml_x_StrConcat(c, a, b);
            assert_eq!((*c).copy_string(joined, usize::MAX), Ok("abcd".to_string()));
            assert_eq!((*c).take_fault(), None);
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn x_float_mod() {
        use crate::rows::eml_x_FloatMod;
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ
        unsafe {
            assert_eq!(eml_x_FloatMod(c, 7.5, 2.0), 1.5);
            assert!(eml_x_FloatMod(c, 1.0, 0.0).is_nan());
            assert_eq!((*c).take_fault(), None);
        }
    }

    #[test]
    fn x_exit_starts_the_abort_and_returns() {
        use crate::rows::eml_x_Exit;
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ。workerの構造体は`with_worker`が置き、その前提を満たす。`exit_abort`は速いモードで失敗せず、
        // 呼び出しは致命的な誤りの道を通らないので、`enter`の中でなくてよい
        unsafe {
            let exit = with_worker(c, |worker| {
                eml_x_Exit(c, 3);
                (*worker).exit
            });
            assert_eq!(exit, Some(3));
            assert_ne!((*c).yielding(), 0);
            assert_eq!((*c).take_fault(), None);
            // 中断の底で、`run`の後始末と同じく中断を終える
            assert_eq!((*c).yield_finish(), Ok(Word::UNIT));
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn x_panic_parks_the_message_and_returns() {
        use crate::rows::eml_x_Panic;
        let mut ctx = ctx();
        let c: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: 上と同じ。workerの構造体は`with_worker`が置き、その前提を満たす。`park_panic`は失敗せず、呼び出しは
        // 致命的な誤りの道を通らないので、`enter`の中でなくてよい。置いたメッセージの参照は取り出して手放す
        unsafe {
            let message = (*c).fresh_string("boom");
            let parked = with_worker(c, |worker| {
                eml_x_Panic(c, message);
                (*worker).panic.take()
            });
            assert_eq!(parked, Some(message));
            assert_eq!((*c).take_fault(), Some(Fault::Engine));
            assert_eq!(
                (*c).copy_string(message, usize::MAX),
                Ok("boom".to_string())
            );
            assert_eq!(live(&mut *c), []);
        }
    }

    #[test]
    fn the_table_has_only_named_prefixes() {
        for entry in crate::entries() {
            let s = entry.symbol;
            assert!(
                ["eml_rt_", "eml_x_", "eml_nrt_"]
                    .iter()
                    .any(|p| s.starts_with(p))
                    || ["eml_enter", "eml_fatal", "eml_run_pending"].contains(&s),
                "{s}"
            );
        }
        let mut names: Vec<_> = crate::entries().map(|e| e.symbol).collect();
        let all = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all);
    }
}
