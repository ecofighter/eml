//! feature `native`のLinuxのビルドで、QtのbackendのC++の部分 (cxxの橋とshim) を組み、Qt 6に動的にリンクする。
//! ほかのビルドでは何もしないので、既定のビルドにQtは要らない (docs/implementation/architecture.mdの「`eml_gui`の内部」)。

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    let native = std::env::var_os("CARGO_FEATURE_NATIVE").is_some();
    let linux = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "linux");
    if native && linux {
        #[cfg(feature = "native")]
        qt::build();
    }
}

#[cfg(feature = "native")]
mod qt {
    use qt_build_utils::QtBuild;

    const SOURCES: [&str; 3] = ["src/qt/mod.rs", "src/qt/shim.hpp", "src/qt/shim.cpp"];

    pub fn build() {
        let modules = ["Core", "Gui", "Widgets"].map(String::from).to_vec();
        // `QMAKE`があればそのqmakeを、なければPATHの`qmake6`か`qmake`を使う
        let qt = QtBuild::new(modules).unwrap_or_else(|e| panic!("eml_gui needs Qt 6: {e}"));
        let version = qt.version();
        if version.major < 6 {
            panic!("eml_gui needs Qt 6 (found Qt {version})");
        }
        let mut build = cxx_build::bridge("src/qt/mod.rs");
        build.file("src/qt/shim.cpp").std("c++17");
        for path in qt.include_paths() {
            build.include(path);
        }
        build.compile("eml_gui_qt");
        // Qtのライブラリを名前でつなぐ (`rustc-link-lib`の既定の種類なので動的である)。rustcはこの順にリンクの引数を
        // 並べ、ld.bfdは左から右に解決するので、Qtはそれを使うshimの静的ライブラリより後に置く
        qt.cargo_link_libraries(&mut build);
        for source in SOURCES {
            println!("cargo::rerun-if-changed={source}");
        }
    }
}
