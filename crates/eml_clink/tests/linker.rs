use eml_clink::{LinkError, Linker, candidates, running_os};
use eml_extern::{CHandleId, CHandleType, CInt, CLibrary, CLink, CParam, CRet, Os};

use crate::common::{c_library, call, function, half};

const MISSING_LIBRARY: &str = "eml-missing-library";
const MISSING_SYMBOL: &str = "eml_missing_symbol";

fn os() -> Os {
    running_os().unwrap()
}

fn named(os: Os, name: &str) -> CLink {
    CLink {
        os,
        library: CLibrary::Named(name.to_owned()),
    }
}

/// 動いている OS でない OS。その OS の `link` の item は開かない。
fn other_os() -> Os {
    if os() == Os::Linux {
        Os::Windows
    } else {
        Os::Linux
    }
}

/// C の標準ライブラリとは別の、どの CI の機械にもあるライブラリ。
fn second_library() -> CLink {
    named(
        os(),
        match os() {
            Os::Linux => "libm.so.6",
            Os::Macos => "libm.dylib",
            Os::Windows => "kernel32",
        },
    )
}

fn llabs(links: Vec<CLink>) -> eml_extern::CFunction {
    function(
        "Test.llabs",
        "llabs",
        links,
        vec![CParam::Int(CInt::I64)],
        CRet::Int(CInt::I64),
    )
}

fn missing(links: Vec<CLink>) -> eml_extern::CFunction {
    function(
        "Test.missing",
        MISSING_SYMBOL,
        links,
        vec![CParam::NoArg],
        CRet::Void,
    )
}

fn handle(dtor: Option<&str>, links: Vec<CLink>) -> CHandleType {
    CHandleType {
        name: "Test.Handle".to_owned(),
        dtor: dtor.map(str::to_owned),
        links,
    }
}

fn quoted(link: &CLink) -> String {
    format!("\"{}\"", link.library.written())
}

#[test]
fn a_registered_symbol_comes_first() {
    // SAFETY: `half` は `double f(double)` で、下の宣言も `Float -> Float` である
    let mut linker =
        unsafe { Linker::new(Vec::new()).with_symbols(&[("llabs", half as *const ())]) };
    let f = function(
        "Test.llabs",
        "llabs",
        vec![c_library()],
        vec![CParam::Float],
        CRet::Float,
    );
    let linked = linker.link(&[f], &[]).unwrap();
    assert_eq!(linked.calls()[0].callee, half as *const ());
    assert_eq!(
        f64::from_bits(call(linked.calls()[0], &[9.0f64.to_bits()])),
        4.5
    );
}

#[test]
fn the_c_library_provides_a_symbol() {
    let mut linker = Linker::new(Vec::new());
    let linked = linker.link(&[llabs(vec![c_library()])], &[]).unwrap();
    assert_eq!(call(linked.calls()[0], &[(-5i64) as u64]), 5);
}

#[test]
fn the_process_provides_a_symbol() {
    let mut linker = Linker::new(Vec::new());
    let linked = linker.link(&[llabs(Vec::new())], &[]).unwrap();
    assert_eq!(call(linked.calls()[0], &[(-6i64) as u64]), 6);
}

#[test]
fn each_function_uses_its_own_links() {
    let mut linker = Linker::new(Vec::new());
    // 別の OS の item は開かない
    let elsewhere = named(other_os(), MISSING_LIBRARY);
    let linked = linker
        .link(
            &[llabs(vec![c_library()]), llabs(vec![elsewhere.clone()])],
            &[],
        )
        .unwrap();
    assert_eq!(linked.calls().len(), 2);
    // 誤りは、その関数の宣言したモジュールの `link` だけを挙げる
    let error = linker
        .link(&[llabs(vec![c_library()]), missing(vec![elsewhere])], &[])
        .unwrap_err();
    assert_eq!(
        error,
        LinkError::Symbol {
            symbol: MISSING_SYMBOL.to_owned(),
            library: None,
        }
    );
    let error = linker
        .link(&[llabs(Vec::new()), missing(vec![second_library()])], &[])
        .unwrap_err();
    assert_eq!(
        error,
        LinkError::Symbol {
            symbol: MISSING_SYMBOL.to_owned(),
            library: Some(quoted(&second_library())),
        }
    );
}

#[test]
fn an_override_replaces_the_written_name() {
    let written = "eml-placeholder";
    let mut linker = Linker::new(vec![(
        written.to_owned(),
        c_library().library.written().to_owned(),
    )]);
    let linked = linker
        .link(&[llabs(vec![named(os(), written)])], &[])
        .unwrap();
    assert_eq!(call(linked.calls()[0], &[(-7i64) as u64]), 7);
}

#[test]
fn an_override_to_a_missing_library_is_a_library_error() {
    let mut linker = Linker::new(vec![(
        c_library().library.written().to_owned(),
        MISSING_LIBRARY.to_owned(),
    )]);
    let error = linker.link(&[llabs(vec![c_library()])], &[]).unwrap_err();
    assert_eq!(
        error,
        LinkError::Library {
            name: MISSING_LIBRARY.to_owned(),
        }
    );
}

#[test]
fn a_missing_library_is_a_link_error() {
    let mut linker = Linker::new(Vec::new());
    // 記号がほかのライブラリにあっても、動いている OS のライブラリはすべて開けなければならない
    let error = linker
        .link(
            &[llabs(vec![c_library(), named(os(), MISSING_LIBRARY)])],
            &[],
        )
        .unwrap_err();
    assert_eq!(
        error,
        LinkError::Library {
            name: MISSING_LIBRARY.to_owned(),
        }
    );
}

#[test]
fn a_missing_symbol_names_the_libraries() {
    let mut linker = Linker::new(Vec::new());
    let error = linker
        .link(&[missing(vec![c_library(), second_library()])], &[])
        .unwrap_err();
    assert_eq!(
        error,
        LinkError::Symbol {
            symbol: MISSING_SYMBOL.to_owned(),
            library: Some(format!(
                "{}, {}",
                quoted(&c_library()),
                quoted(&second_library())
            )),
        }
    );
}

#[test]
fn a_missing_symbol_without_libraries_names_none() {
    let mut linker = Linker::new(Vec::new());
    let error = linker.link(&[missing(Vec::new())], &[]).unwrap_err();
    assert_eq!(
        error,
        LinkError::Symbol {
            symbol: MISSING_SYMBOL.to_owned(),
            library: None,
        }
    );
}

extern "C" fn test_dtor(_: *mut core::ffi::c_void) {}

#[test]
fn a_dtor_resolves_like_a_function() {
    // SAFETY: `test_dtor` は `void f(void*)` で、dtor としてだけ引く
    let mut linker = unsafe {
        Linker::new(Vec::new()).with_symbols(&[("eml_test_dtor", test_dtor as *const ())])
    };
    let handles = [
        handle(Some("eml_test_dtor"), Vec::new()),
        handle(None, vec![c_library()]),
        handle(Some("free"), vec![c_library()]),
    ];
    let linked = linker.link(&[], &handles).unwrap();
    assert!(linked.calls().is_empty());
    assert_eq!(
        linked.dtors()[0].map(|dtor| dtor as usize),
        Some(test_dtor as *const () as usize)
    );
    assert!(linked.dtors()[1].is_none());
    assert!(linked.dtors()[2].is_some());
    // 関数とハンドルを一緒につなぐと、それぞれの表に並びをそろえる
    let f = function(
        "Test.make",
        "llabs",
        vec![c_library()],
        vec![CParam::Int(CInt::I64)],
        CRet::Handle(CHandleId(0)),
    );
    let linked = linker.link(&[f], &handles[..1]).unwrap();
    assert_eq!((linked.calls().len(), linked.dtors().len()), (1, 1));
}

#[test]
fn a_missing_dtor_is_a_symbol_error() {
    let mut linker = Linker::new(Vec::new());
    let error = linker
        .link(&[], &[handle(Some(MISSING_SYMBOL), vec![c_library()])])
        .unwrap_err();
    assert_eq!(
        error,
        LinkError::Symbol {
            symbol: MISSING_SYMBOL.to_owned(),
            library: Some(quoted(&c_library())),
        }
    );
}

#[test]
fn names_are_adorned_per_os() {
    let name = |s: &str| CLibrary::Named(s.to_owned());
    assert_eq!(candidates(Os::Linux, &name("m")), ["libm.so"]);
    assert_eq!(candidates(Os::Macos, &name("m")), ["libm.dylib"]);
    assert_eq!(candidates(Os::Windows, &name("m")), ["m.dll"]);
    for written in ["libc.so.6", "lib/x", "C:\\x", "./y"] {
        for os in [Os::Linux, Os::Macos, Os::Windows] {
            assert_eq!(candidates(os, &name(written)), [written]);
        }
    }
}

#[test]
fn a_framework_tries_the_system_folder_first() {
    assert_eq!(
        candidates(Os::Macos, &CLibrary::Framework("Accelerate".to_owned())),
        [
            "/System/Library/Frameworks/Accelerate.framework/Accelerate",
            "/Library/Frameworks/Accelerate.framework/Accelerate",
        ]
    );
}

#[test]
fn link_error_texts() {
    let texts = [
        LinkError::Library {
            name: "blas".to_owned(),
        },
        LinkError::Symbol {
            symbol: "f".to_owned(),
            library: None,
        },
        LinkError::Symbol {
            symbol: "f".to_owned(),
            library: Some("\"a\", \"b\"".to_owned()),
        },
        LinkError::UnknownOverride {
            key: "blas".to_owned(),
        },
        LinkError::Unsupported {
            reason: "no JIT".to_owned(),
        },
    ]
    .map(|error| error.to_string());
    assert_eq!(
        texts,
        [
            "library \"blas\" was not found",
            "symbol \"f\" was not found",
            "symbol \"f\" was not found in \"a\", \"b\"",
            "--link names no library \"blas\"",
            "C functions cannot be called on this machine: no JIT",
        ]
    );
}

#[test]
fn the_linker_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Linker>();
}

#[cfg(target_os = "macos")]
#[test]
fn accelerate_loads_as_a_framework() {
    let accelerate = CLink {
        os: Os::Macos,
        library: CLibrary::Framework("Accelerate".to_owned()),
    };
    let ddot = function(
        "Test.ddot",
        "cblas_ddot",
        vec![accelerate],
        vec![
            CParam::Int(CInt::I32),
            CParam::UnboxedArray,
            CParam::Int(CInt::I32),
            CParam::UnboxedArray,
            CParam::Int(CInt::I32),
        ],
        CRet::Float,
    );
    let linked = Linker::new(Vec::new()).link(&[ddot], &[]).unwrap();
    let x = [1.0f64, 2.0, 3.0];
    let y = [4.0f64, 5.0, 6.0];
    let args = [
        3,
        x.as_ptr().expose_provenance() as u64,
        1,
        y.as_ptr().expose_provenance() as u64,
        1,
    ];
    assert_eq!(f64::from_bits(call(linked.calls()[0], &args)), 32.0);
}
