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

/// 引数と結果のRustの型。`i64`と`f64`のほかに、行の表が書くReprの名前も受ける。ほかのトークンはコンパイルの誤りになるので、
/// どの引数も`i64`か`f64`である (docs/spec/native.md の「エントリのABI」)。
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
        i64
    };
    (tobj) => {
        i64
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

    use eml_rt::{Buffering, Control, Ctx, Fast, HostRef, Limits, StdHost};

    use crate::{Fatal, Kind};

    crate::entries! {
        const SAMPLE;

        plain fn eml_t_plain(ctx, a: i64, b: f64) -> f64 {
            // SAFETY: テストは生きている`Ctx`を渡す
            let limit = unsafe { (*ctx).stack_limit() };
            a as f64 + b + limit as f64
        }

        shim fn eml_t_shim(ctx, a: i64) -> i64 {
            ctx.set_stack_limit(a as usize);
            Ok::<i64, Fatal>(a + 1)
        }

        #[allow(non_snake_case)]
        shim fn eml_t_Named(ctx, a: obj, b: float, c: enum) -> int {
            ctx.set_stack_limit(a as usize);
            Ok::<i64, Fatal>(a + b as i64 + c)
        }

        #[allow(non_snake_case)]
        shim fn eml_t_Never(ctx, a: tobj) -> never {
            ctx.set_stack_limit(a as usize);
            Ok::<(), Fatal>(())
        }

        shim fn eml_t_unit(ctx, a: i64) {
            ctx.set_stack_limit(a as usize);
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
            assert_eq!(eml_t_Named(raw, 1, 2.0, 3), 6);
            eml_t_Never(raw, 9);
            eml_t_unit(raw, 6);
            assert_eq!(eml_t_plain(raw, 1, 0.5), 7.5);
        }
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
