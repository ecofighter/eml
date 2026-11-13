//! stdのexternの行のエントリ`eml_x_*` (docs/spec/native.md の「externの失敗」)。行の一覧`extern_rows!`の展開`entries`が、
//! `inline`でない行の変種ごとに、共有の行の関数を呼ぶ`shim`のエントリを作る。`Exit`と`Panic`はエンジンごとに違うので、
//! ここで手で書く。
#![allow(unsafe_code)]

use eml_rt::HeapError;

use crate::enter::Fatal;
use crate::worker;

/// 行の結果をエントリの結果にする。`unit`の行の`()`はUnitの語に、`never`の行の`Infallible`は結果なしになる。
macro_rules! entry_value {
    (unit, $v:ident) => {{
        let () = $v;
        $crate::__rt::Word::UNIT.bits() as i64
    }};
    (never, $v:ident) => {
        match $v {}
    };
    ($kind:tt, $v:ident) => {
        $v
    };
}

/// 失敗を置いたエントリが返す仮の値。生成したコードはすぐ後でfaultの語を確かめるので、この値を使わない。物体の語の
/// 位置には即値を返す。
macro_rules! placeholder {
    (obj) => {
        $crate::__rt::Word::UNIT
    };
    (tobj) => {
        $crate::__rt::Word::UNIT
    };
    (unit) => {
        $crate::__rt::Word::UNIT.bits() as i64
    };
    (int) => {
        0i64
    };
    (enum) => {
        0i64
    };
    (float) => {
        0.0f64
    };
    (never) => {
        ()
    };
}

/// 一覧の展開。`inline`の行を飛ばし、ほかの行の変種ごとに1つのエントリを作る。行の本体は読まず、一覧の最初の変種の
/// 名前の行の関数を`$rows`から呼ぶ。本体は`eml_rows`の中の名前を修飾せずに書くためである。作ったエントリは、
/// 最後に`$done!`へ渡して1つの表にする。
macro_rules! entries {
    (
        generic { $($generic:tt)* }
        dedicated { $($dedicated:tt)* }
        engine { $($engine:tt)* }
        lir { $($lir:tt)* }
        task { $($task:tt)* }
    ) => {
        entries!(@lines [eml_rows::rows] rows_table [] $($generic)* $($dedicated)*);
    };
    (@lines [$($rows:ident)::+] $done:ident [$($acc:tt)*]) => {
        $done! { $($acc)* }
    };
    (@lines $rows:tt $done:ident $acc:tt
        inline fn $first:ident $(| $rest:ident)* ( $ctx:tt $(, $arg:ident : $kind:tt)* ) -> $ret:tt $body:block
        $($tail:tt)*
    ) => {
        entries!(@lines $rows $done $acc $($tail)*);
    };
    (@lines $rows:tt $done:ident $acc:tt
        fn $first:ident as $entry:ident $(| $rest:ident as $rest_entry:ident)*
        ( $ctx:tt $(, $arg:ident : $kind:tt)* ) -> $ret:tt $body:block
        $($tail:tt)*
    ) => {
        entries!(@variants $rows $done $acc $first [$first $entry $(, $rest $rest_entry)*]
            ($($arg : $kind),*) $ret [$($tail)*]);
    };
    (@variants $rows:tt $done:ident $acc:tt $first:ident [] $args:tt $ret:tt [$($tail:tt)*]) => {
        entries!(@lines $rows $done $acc $($tail)*);
    };
    (@variants [$($rows:ident)::+] $done:ident [$($acc:tt)*] $first:ident
        [$variant:ident $entry:ident $(, $more:ident $more_entry:ident)*]
        ($($arg:ident : $kind:tt),*) $ret:tt $tail:tt
    ) => {
        entries!(@variants [$($rows)::+] $done [
            $($acc)*
            #[allow(non_snake_case)]
            pub(crate) shim fn $entry(ctx $(, $arg: $kind)*) -> $ret {
                // SAFETY: 生成したコードは、行の関数が求めるとおりの語を渡す (docs/spec/lir.md の「extern の借用」)
                match unsafe { $($rows)::+::$first(ctx $(, $arg)*) } {
                    Ok(value) => Ok(entry_value!($ret, value)),
                    Err(::eml_sched::error::RunFault::Extern(fault))
                        if const { ::eml_extern::Extern::$variant.row().faults } =>
                    {
                        let previous = ctx.set_fault(fault);
                        debug_assert!(previous.is_none(), "a fault was already parked");
                        Ok(placeholder!($ret))
                    }
                    // 内部の類の誤りは、`Err`が`shim`に届く前にworkerの構造体へ移す (docs/spec/native.md の「externの失敗」)
                    // SAFETY: 行のエントリは、`ctx.runtime()`が生きている`Worker`を指し、呼び出しの間ほかの誰もそれに
                    // 触れないときだけ呼ぶ (docs/spec/native.md の「実行の始めと終わり」)
                    Err(error) => Err(unsafe { $crate::worker::record(ctx, error, None) }),
                }
            }
        ] $first [$($more $more_entry),*] ($($arg : $kind),*) $ret $tail);
    };
}

/// 一覧のエントリに`Exit`と`Panic`を足して、表`ROWS`にする。
macro_rules! rows_table {
    ($($rows:tt)*) => {
        crate::entries! {
            pub(crate) const ROWS;

            $($rows)*

            /// 終了コードをworkerの構造体に置き、`exit`の中断を始めて戻る。生成したコードは、戻った後で巻き戻しへ分岐する。
            #[allow(non_snake_case)]
            pub(crate) shim fn eml_x_Exit(ctx, code: int) -> never {
                // SAFETY: 行のエントリは、`ctx.runtime()`が生きている`Worker`を指し、呼び出しの間ほかの誰もそれに触れない
                // ときだけ呼ぶ (docs/spec/native.md の「実行の始めと終わり」)
                let worker = unsafe { worker::of(ctx) };
                debug_assert!(worker.exit.is_none(), "an exit code was already set");
                worker.exit = Some(code);
                // SAFETY: 生成したコードは、中断の途中でexternを呼ばない
                unsafe { ctx.exit_abort() }?;
                Ok::<(), HeapError>(())
            }

            /// メッセージをworkerの構造体に置いて戻る。`message`の参照はworkerの構造体へ移るので、生成したコードは
            /// 呼び出しの後でこれを手放さない。
            #[allow(non_snake_case)]
            pub(crate) shim fn eml_x_Panic(ctx, message: obj) -> never {
                // SAFETY: 行のエントリは、`ctx.runtime()`が生きている`Worker`を指し、呼び出しの間ほかの誰もそれに触れない
                // ときだけ呼ぶ (docs/spec/native.md の「実行の始めと終わり」)。
                // 生成したコードは、生きている文字列の参照を1つ渡し、その後この語を使わない。失敗を置いたままでは呼ばない
                unsafe { worker::park_panic(ctx, message) };
                Ok::<(), Fatal>(())
            }
        }
    };
}

eml_rows::extern_rows!(entries);

/// 致命的な誤りの道を通るので、Miriでは流さない (docs/implementation/testing.md の「`eml_nrt`の検証」)。
#[cfg(all(test, not(miri)))]
mod fatal {
    use std::sync::Arc;

    use eml_rt::{Buffering, Control, Ctx, Fast, HostRef, Limits, Mode, StdHost, Word};
    use eml_sched::error::{RunFault, RuntimeError};

    use crate::enter::{FATAL_RECORDED, Fatal, enter};
    use crate::image::{Image, Slice};
    use crate::worker::{NATIVE_CODE, Worker};

    /// テストだけの行の関数。内部の類の誤りを返す。
    mod test_rows {
        use super::*;

        #[allow(non_snake_case)]
        pub unsafe fn IntShow<M: Mode>(_: &mut Ctx<M>, _: i64) -> Result<Word, RunFault> {
            Err(RunFault::Internal("a test row failed"))
        }
    }

    macro_rules! test_table {
        ($($rows:tt)*) => {
            crate::entries! {
                const TEST_ROWS;
                $($rows)*
            }
        };
    }

    entries!(@lines [test_rows] test_table []
        fn IntShow as eml_x_IntShow(ctx, n: int) -> obj {
            Err(RunFault::Internal("a test row failed"))
        }
    );

    unsafe extern "C" fn call_row(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        // SAFETY: `enter`は生きている`Ctx`を渡し、`argv`はテストの語を指す。エントリは戻らない
        unsafe { *argv = eml_x_IntShow(ctx, 5).bits() as u64 };
    }

    #[test]
    fn a_row_error_of_the_internal_class_is_recorded() {
        assert_eq!(TEST_ROWS.len(), 1);
        let host = StdHost::new(
            Box::new(std::io::sink()),
            Box::new(std::io::sink()),
            std::path::PathBuf::new(),
            Buffering::Block,
        );
        let control = Arc::new(Control::new(Limits::default()));
        let mut ctx = Box::new(Ctx::<Fast>::new(control, HostRef::new(Arc::new(host))));
        let image = Image {
            functions: Slice::new(&[]),
            files: Slice::new(&[]),
            data: Slice::new(&[]),
            paps: Slice::new(&[]),
        };
        // SAFETY: `image`の表は空で、workerより長く生きる
        let mut worker = unsafe { Worker::new(&image) };
        ctx.set_runtime((&raw mut worker).cast());
        let mut argv = [0u64];
        let raw: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: `runtime`は`run`と同じく、`enter`の間だけ生きているworkerの構造体を指す。`ctx`と`worker`への参照を
        // エントリをまたいで持たないので、エントリの間ほかの誰もworkerに触れない。エントリは致命的な誤りの道を通るので、
        // `enter`の中で呼ぶ
        let entered = unsafe { enter(raw, call_row, std::ptr::null(), argv.as_mut_ptr()) };
        ctx.set_runtime(std::ptr::null_mut());
        assert_eq!(
            entered,
            Err(Fatal {
                code: FATAL_RECORDED,
                site: 0
            })
        );
        assert_eq!(argv, [0]);
        assert_eq!(
            worker.error,
            Some(RuntimeError::Fault {
                fault: RunFault::Internal("a test row failed"),
                function: NATIVE_CODE.to_string(),
                at: None,
            })
        );
    }
}
