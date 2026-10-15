//! REPL のセッションの読み込みと名前解決 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の
//! 「セッションのモジュール」)。

use eml_diagnostics::sort_diagnostics;
use eml_hir::{
    DefMap, ImportCache, ImportTarget, Loaded, ModuleId, ModuleOrigin, ModulePath, Program,
    SessionInput,
};
use eml_test_support::{MemorySource, located};

/// 入力は (入力の番号, テキスト)。
fn inputs<'a>(texts: &[(u32, &'a str)]) -> Vec<SessionInput<'a>> {
    texts
        .iter()
        .map(|&(number, text)| SessionInput { number, text })
        .collect()
}

/// セッションを読み込み、診断を `パス 行:列 番号 メッセージ` の行にする。
fn load(
    texts: &[(u32, &str)],
    modules: &[(&str, &str)],
    cache: &ImportCache,
) -> (Loaded, Vec<String>, ImportCache) {
    let (loaded, mut diagnostics, read) = eml_hir::load_session(
        eml_hir::STD,
        &inputs(texts),
        "",
        &MemorySource(modules),
        cache,
    );
    sort_diagnostics(&mut diagnostics);
    let lines = diagnostics
        .iter()
        .map(|d| {
            format!(
                "{} {} {}",
                located(&loaded.files, &d.primary),
                d.code,
                d.message
            )
        })
        .collect();
    (loaded, lines, read)
}

fn names(loaded: &Loaded) -> Vec<&str> {
    loaded
        .modules
        .iter()
        .map(|module| module.name.as_str())
        .collect()
}

fn origins(loaded: &Loaded) -> Vec<ModuleOrigin> {
    loaded.modules.iter().map(|module| module.origin).collect()
}

fn index(module: ModuleId) -> usize {
    u32::from(module.into_raw()) as usize
}

fn module_named(loaded: &Loaded, name: &str) -> ModuleId {
    let index = loaded
        .modules
        .iter()
        .position(|module| module.name == name)
        .unwrap_or_else(|| panic!("no module `{name}`"));
    ModuleId::from_raw(la_arena::RawIdx::from(index as u32))
}

/// 読み込み、def_map、lower を流したプログラム。診断は3つの段を合わせて行にする。
struct Lowered {
    loaded: Loaded,
    def_map: DefMap,
    program: Program,
    diagnostics: Vec<String>,
}

fn lower(texts: &[(u32, &str)], modules: &[(&str, &str)]) -> Lowered {
    let (loaded, mut diagnostics, _) = eml_hir::load_session(
        eml_hir::STD,
        &inputs(texts),
        "",
        &MemorySource(modules),
        &ImportCache::default(),
    );
    let (def_map, stage) = eml_hir::def_map(&loaded.modules);
    diagnostics.extend(stage);
    let (program, stage) = eml_hir::lower(&def_map, &loaded.modules);
    diagnostics.extend(stage);
    sort_diagnostics(&mut diagnostics);
    let diagnostics = diagnostics
        .iter()
        .map(|d| {
            format!(
                "{} {} {}",
                located(&loaded.files, &d.primary),
                d.code,
                d.message
            )
        })
        .collect();
    Lowered {
        loaded,
        def_map,
        program,
        diagnostics,
    }
}

#[test]
fn session_modules_follow_the_prelude_in_input_order() {
    let (loaded, diagnostics, _) = load(
        &[(1, "a : Int\na = 1"), (3, "b : Int\nb = 2")],
        &[],
        &ImportCache::default(),
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(
        names(&loaded),
        ["Prelude", "<input 1>", "<input 3>", "Std.Fs"]
    );
    assert_eq!(
        origins(&loaded),
        [
            ModuleOrigin::Std,
            ModuleOrigin::Session(1),
            ModuleOrigin::Session(3),
            ModuleOrigin::Std
        ]
    );
    // 今の入力 (最後の入力) が入口である
    assert_eq!(loaded.files.path(loaded.entry), "<input 3>");
}

#[test]
fn imports_of_inputs_come_after_the_std_modules() {
    let (loaded, diagnostics, _) = load(
        &[(1, "import A"), (2, "import B")],
        &[("A.em", ""), ("B.em", "")],
        &ImportCache::default(),
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(
        names(&loaded),
        ["Prelude", "<input 1>", "<input 2>", "Std.Fs", "A", "B"]
    );
}

#[test]
fn the_import_cache_keeps_the_first_text() {
    let (_, diagnostics, first) = load(
        &[(1, "import A")],
        &[("A.em", "import B\npub x : Int\nx = 1"), ("B.em", "")],
        &ImportCache::default(),
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    // 間接の依存先も持つ
    assert!(first.contains(&ModulePath(vec!["A".to_string()])));
    assert!(first.contains(&ModulePath(vec!["B".to_string()])));
    // ディスクのファイルが変わっても、キャッシュのテキストを使い、新しく読んだものはない
    let (loaded, diagnostics, second) = load(
        &[(1, "import A"), (2, "y : Int\ny = A.x")],
        &[("A.em", "pub z : Int\nz = 2")],
        &first,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(second.is_empty());
    let a = module_named(&loaded, "A");
    assert_eq!(
        loaded.files.text(loaded.modules[index(a)].tree.file),
        "import B\npub x : Int\nx = 1"
    );
}

#[test]
fn a_cached_fallback_to_the_standard_library_stays() {
    let (loaded, diagnostics, first) = load(&[(1, "import Fs")], &[], &ImportCache::default());
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(
        loaded.modules[1].targets,
        [ImportTarget::Module(module_named(&loaded, "Std.Fs"))]
    );
    // 後でユーザーの根に `Fs.em` ができても、セッションの `Fs` は標準ライブラリのままである
    let (loaded, diagnostics, _) = load(
        &[(1, "import Fs")],
        &[("Fs.em", "pub f : Int\nf = 1")],
        &first,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(names(&loaded), ["Prelude", "<input 1>", "Std.Fs"]);
    assert_eq!(
        loaded.modules[1].targets,
        [ImportTarget::Module(module_named(&loaded, "Std.Fs"))]
    );
}

#[test]
fn a_module_that_cannot_be_read_is_not_cached() {
    let (_, diagnostics, read) = load(&[(1, "import Missing")], &[], &ImportCache::default());
    assert_eq!(
        diagnostics,
        ["<input 1> 1:8 E1026 cannot find module `Missing`"]
    );
    assert!(read.is_empty());
}

#[test]
fn a_session_has_no_main() {
    let lowered = lower(&[(1, "main : Unit -> Unit\nmain () = ()")], &[]);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    assert_eq!(
        lowered.def_map.entry(),
        module_named(&lowered.loaded, "<input 1>")
    );
    assert_eq!(lowered.program.entry, lowered.def_map.entry());
    // セッションでは `check_main` を当てないので、`main` を入口の関数として探さない
    assert_eq!(lowered.program.main(), None);
}
