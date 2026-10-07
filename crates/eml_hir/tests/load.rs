//! 読み込みの段 (docs/implementation/architecture.md の「`eml_hir` の内部」)。

use eml_diagnostics::sort_diagnostics;
use eml_hir::{ImportTarget, Loaded, ModulePath, ModuleSource, ReadError};
use eml_test_support::{ENTRY_PATH, MemorySource, full};

/// 読み込みの段を流し、診断を `パス 行:列 番号 メッセージ` の行にする。依存先のファイルを指す診断を読み分けるため。
fn load_at(entry_path: &str, entry: &str, modules: &[(&str, &str)]) -> (Loaded, Vec<String>) {
    let (loaded, mut diagnostics) = eml_hir::load(entry_path, entry, &MemorySource(modules));
    sort_diagnostics(&mut diagnostics);
    let lines = diagnostics
        .iter()
        .map(|d| {
            let file = d.primary.file;
            format!(
                "{} {} {} {}",
                loaded.files.path(file),
                loaded.files.line_col(file, d.primary.range.start()),
                d.code,
                d.message
            )
        })
        .collect();
    (loaded, lines)
}

fn load(entry: &str, modules: &[(&str, &str)]) -> (Loaded, Vec<String>) {
    load_at(ENTRY_PATH, entry, modules)
}

fn names(loaded: &Loaded) -> Vec<&str> {
    loaded
        .modules
        .iter()
        .map(|module| module.name.as_str())
        .collect()
}

fn paths(loaded: &Loaded) -> Vec<&str> {
    loaded
        .modules
        .iter()
        .map(|module| loaded.files.path(module.tree.file))
        .collect()
}

/// `index` 番のモジュールの import が指すモジュールの番号。壊れた import は `None`。
fn targets(loaded: &Loaded, index: usize) -> Vec<Option<u32>> {
    loaded.modules[index]
        .targets
        .iter()
        .map(|target| match target {
            ImportTarget::Module(id) => Some(u32::from(id.into_raw())),
            ImportTarget::Broken => None,
        })
        .collect()
}

#[test]
fn modules_are_numbered_in_breadth_first_order() {
    let (loaded, diagnostics) = load_at(
        "app/main.em",
        "import B\nimport A.C",
        &[
            ("B.em", "import D"),
            ("A/C.em", "import D\nimport B"),
            ("D.em", ""),
        ],
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(names(&loaded), ["Prelude", "Main", "B", "A.C", "D"]);
    // 表示のパスは、入口の表示のパスのディレクトリに根からの相対パスをつないだもの
    assert_eq!(
        paths(&loaded),
        [
            "Prelude.em",
            "app/main.em",
            "app/B.em",
            "app/A/C.em",
            "app/D.em"
        ]
    );
    assert_eq!(loaded.files.path(loaded.prelude), "Prelude.em");
    assert_eq!(loaded.files.path(loaded.entry), "app/main.em");
    assert_eq!(targets(&loaded, 0), Vec::<Option<u32>>::new());
    assert_eq!(targets(&loaded, 1), [Some(2), Some(3)]);
    assert_eq!(targets(&loaded, 2), [Some(4)]);
    assert_eq!(targets(&loaded, 3), [Some(4), Some(2)]);
}

#[test]
fn an_entry_without_a_directory_uses_the_root_relative_path() {
    let (loaded, diagnostics) = load("import Report.Csv", &[("Report/Csv.em", "")]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(names(&loaded), ["Prelude", "Main", "Report.Csv"]);
    assert_eq!(&paths(&loaded)[1..], ["test.em", "Report/Csv.em"]);
}

#[test]
fn a_module_imported_twice_is_loaded_once() {
    let (loaded, diagnostics) = load("import A\nimport A as X", &[("A.em", "")]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(names(&loaded), ["Prelude", "Main", "A"]);
    assert_eq!(targets(&loaded, 1), [Some(2), Some(2)]);
}

#[test]
fn a_cycle_loads_each_module_once() {
    // 循環の報告 (E1027) は `def_map` が行う。読み込みの段は止まればよい
    let (loaded, diagnostics) = load("import A", &[("A.em", "import B"), ("B.em", "import A")]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(names(&loaded), ["Prelude", "Main", "A", "B"]);
    assert_eq!(targets(&loaded, 3), [Some(2)]);
}

#[test]
fn a_missing_module_is_reported_at_its_path() {
    let (loaded, mut diagnostics) = eml_hir::load(
        "app/main.em",
        "import Report.Csv\nimport M",
        &MemorySource(&[]),
    );
    sort_diagnostics(&mut diagnostics);
    insta::assert_snapshot!(full(&loaded.files, &diagnostics), @r"
    E1026 1:8 cannot find module `Report.Csv`
      1:8 there is no file `app/Report/Csv.em`
    E1026 2:8 cannot find module `M`
      2:8 there is no file `app/M.em`
    ");
    assert_eq!(names(&loaded), ["Prelude", "Main"]);
    assert_eq!(targets(&loaded, 1), [None, None]);
}

/// どのモジュールも読めない読み方。
struct Unreadable;

impl ModuleSource for Unreadable {
    fn read(&self, _: &ModulePath) -> Result<String, ReadError> {
        Err(ReadError::Unreadable(
            "stream did not contain valid UTF-8".to_string(),
        ))
    }
}

#[test]
fn an_unreadable_module_reports_the_reason() {
    let (loaded, diagnostics) = eml_hir::load(ENTRY_PATH, "import A", &Unreadable);
    assert_eq!(
        full(&loaded.files, &diagnostics),
        "E1026 1:8 cannot read module `A`: stream did not contain valid UTF-8\n  1:8 `A.em` cannot be read\n"
    );
    assert_eq!(targets(&loaded, 1), [None]);
}

#[test]
fn reserved_modules_and_the_entry_cannot_be_imported() {
    // 根に同じ名前のファイルがあっても読まない
    let text = "import Prelude\nimport Prelude as P\nimport Util.Prelude\nimport A as Prelude\nimport Main\nimport Server";
    let modules = [
        ("Prelude.em", ""),
        ("Util/Prelude.em", ""),
        ("A.em", ""),
        ("Main.em", ""),
        ("Server.em", ""),
    ];
    let (loaded, mut diagnostics) = eml_hir::load("app/Server.em", text, &MemorySource(&modules));
    sort_diagnostics(&mut diagnostics);
    insta::assert_snapshot!(full(&loaded.files, &diagnostics), @r"
    E1030 1:1 the module `Prelude` is reserved
      1:1 the Prelude is imported implicitly
    E1030 2:1 the module `Prelude` is reserved
      2:1 the Prelude is imported implicitly
    E1030 3:1 the qualifier `Prelude` is reserved
      3:1 `Prelude` always qualifies the names of the Prelude
      help: choose another qualifier with `as`
    E1030 4:1 the qualifier `Prelude` is reserved
      4:1 `Prelude` always qualifies the names of the Prelude
      help: choose another qualifier with `as`
    E1030 5:1 the module `Main` is reserved
      5:1 `Main` is the name of the entry module
    E1030 6:1 the entry module cannot be imported
      6:1 `app/Server.em` is the entry file
    ");
    assert_eq!(names(&loaded), ["Prelude", "Main"]);
    assert_eq!(targets(&loaded, 1), [None; 6]);
}

#[test]
fn a_dependency_cannot_import_the_entry() {
    let (loaded, diagnostics) = load_at("app/Server.em", "import A", &[("A.em", "import Server")]);
    assert_eq!(
        diagnostics,
        ["app/A.em 1:1 E1030 the entry module cannot be imported"]
    );
    assert_eq!(targets(&loaded, 2), [None]);
}

#[test]
fn diagnostics_of_a_dependency_point_into_its_file() {
    let (loaded, diagnostics) = load("import A", &[("A.em", "import Missing\nh = 3")]);
    assert_eq!(
        diagnostics,
        [
            "A.em 1:8 E1026 cannot find module `Missing`",
            "A.em 2:1 E1004 `h` has no type signature",
        ]
    );
    assert_eq!(targets(&loaded, 2), [None]);
}

#[test]
fn an_import_after_a_declaration_is_still_loaded() {
    // 報告するのはパーサの E0011 だけで、ほかの import と同じく読み込む
    let (loaded, diagnostics) = load("f : Int\nf = 1\nimport A", &[("A.em", "")]);
    assert_eq!(
        diagnostics,
        ["test.em 3:1 E0011 imports must come before declarations"]
    );
    assert_eq!(names(&loaded), ["Prelude", "Main", "A"]);
}

#[test]
fn a_missing_module_imported_after_a_declaration_is_reported() {
    // 宣言の後の import も読みに行くので、見つからなければ E0011 に E1026 が続く
    let (loaded, diagnostics) = load("f : Int\nf = 1\nimport Missing", &[]);
    assert_eq!(
        diagnostics,
        [
            "test.em 3:1 E0011 imports must come before declarations",
            "test.em 3:8 E1026 cannot find module `Missing`",
        ]
    );
    assert_eq!(targets(&loaded, 1), [None]);
}
