//! 読み込みの段 (docs/implementation/architecture.md の「`eml_hir` の内部」)。

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use eml_diagnostics::sort_diagnostics;
use eml_hir::{
    DefMap, ImportTarget, Loaded, ModuleId, ModuleOrigin, ModulePath, ModuleSource, NameRef,
    Program, ReadError, Resolved, ValueItem,
};
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
    assert_eq!(
        names(&loaded),
        ["Prelude", "Main", "Std.Fs", "B", "A.C", "D"]
    );
    // 表示のパスは、入口の表示のパスのディレクトリに根からの相対パスをつないだもの
    assert_eq!(
        paths(&loaded),
        [
            "<std>/Prelude.em",
            "app/main.em",
            "<std>/Fs.em",
            "app/B.em",
            "app/A/C.em",
            "app/D.em"
        ]
    );
    assert_eq!(loaded.files.path(loaded.prelude), "<std>/Prelude.em");
    assert_eq!(loaded.files.path(loaded.entry), "app/main.em");
    assert_eq!(targets(&loaded, 0), Vec::<Option<u32>>::new());
    assert_eq!(targets(&loaded, 1), [Some(3), Some(4)]);
    assert_eq!(targets(&loaded, 3), [Some(5)]);
    assert_eq!(targets(&loaded, 4), [Some(5), Some(3)]);
}

#[test]
fn an_entry_without_a_directory_uses_the_root_relative_path() {
    let (loaded, diagnostics) = load("import Report.Csv", &[("Report/Csv.em", "")]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(names(&loaded), ["Prelude", "Main", "Std.Fs", "Report.Csv"]);
    assert_eq!(
        &paths(&loaded)[1..],
        ["test.em", "<std>/Fs.em", "Report/Csv.em"]
    );
}

#[test]
fn a_module_imported_twice_is_loaded_once() {
    let (loaded, diagnostics) = load("import A\nimport A as X", &[("A.em", "")]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(names(&loaded), ["Prelude", "Main", "Std.Fs", "A"]);
    assert_eq!(targets(&loaded, 1), [Some(3), Some(3)]);
}

#[test]
fn a_cycle_loads_each_module_once() {
    // 循環の報告 (E1027) は `def_map` が行う。読み込みの段は止まればよい
    let (loaded, diagnostics) = load("import A", &[("A.em", "import B"), ("B.em", "import A")]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(names(&loaded), ["Prelude", "Main", "Std.Fs", "A", "B"]);
    assert_eq!(targets(&loaded, 4), [Some(3)]);
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
    assert_eq!(names(&loaded), ["Prelude", "Main", "Std.Fs"]);
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
    let text = "import Prelude\nimport Prelude as P\nimport Util.Prelude\nimport A as Prelude\nimport Main\nimport Server\nimport Std.Prelude\nimport Std.Prelude as P";
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
    E1030 7:1 the module `Std.Prelude` is reserved
      7:1 the Prelude is imported implicitly
    E1030 8:1 the module `Std.Prelude` is reserved
      8:1 the Prelude is imported implicitly
    ");
    assert_eq!(names(&loaded), ["Prelude", "Main", "Std.Fs"]);
    assert_eq!(targets(&loaded, 1), [None; 8]);
}

#[test]
fn a_dependency_cannot_import_the_entry() {
    let (loaded, diagnostics) = load_at("app/Server.em", "import A", &[("A.em", "import Server")]);
    assert_eq!(
        diagnostics,
        ["app/A.em 1:1 E1030 the entry module cannot be imported"]
    );
    assert_eq!(targets(&loaded, 3), [None]);
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
    assert_eq!(targets(&loaded, 3), [None]);
}

#[test]
fn an_import_after_a_declaration_is_still_loaded() {
    // 報告するのはパーサの E0011 だけで、ほかの import と同じく読み込む
    let (loaded, diagnostics) = load("f : Int\nf = 1\nimport A", &[("A.em", "")]);
    assert_eq!(
        diagnostics,
        ["test.em 3:1 E0011 imports must come before declarations"]
    );
    assert_eq!(names(&loaded), ["Prelude", "Main", "Std.Fs", "A"]);
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

#[test]
fn an_import_with_a_malformed_path_reads_no_module() {
    // パスの後ろ (別名と並びを含む) に構文の誤りがある import は壊れた import で、どのファイルも読まない。報告するのは
    // 構文の誤りだけである (docs/implementation/architecture.md の「名前解決の回復」)
    let present: &[(&str, &str)] = &[("Report.em", "pub x : Int\nx = 1")];
    for (entry, modules) in [
        ("import Report.\n", &[][..]),
        ("import Report.\n", present),
        ("import Report.csv\n", present),
        ("import Report as\n", present),
        ("import Report as 1\n", present),
        ("import Report (x, 1)\n", present),
        ("import Report (x y)\n", present),
        ("import Report (x\n", present),
    ] {
        let (loaded, diagnostics) = load(entry, modules);
        assert_eq!(names(&loaded), ["Prelude", "Main", "Std.Fs"], "{entry}");
        assert_eq!(targets(&loaded, 1), [None], "{entry}");
        assert_eq!(diagnostics.len(), 1, "{entry}: {diagnostics:?}");
        assert!(
            diagnostics[0].contains(" E0011 "),
            "{entry}: {diagnostics:?}"
        );
    }
}

#[test]
fn an_infix_constructor_in_an_import_list_still_loads_the_module() {
    // `(:+)` は E0011 だが、その名前だけを落とす誤りなので、import そのものは壊れない
    // (docs/implementation/architecture.md の「名前解決の回復」)
    let present: &[(&str, &str)] = &[("Report.em", "pub x : Int\nx = 1")];
    let (loaded, diagnostics) = load("import Report ((:+), x)\n", present);
    assert_eq!(names(&loaded), ["Prelude", "Main", "Std.Fs", "Report"]);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].contains(" E0011 "), "{diagnostics:?}");
}

/// 本物の `Fs` に関数を足したもの。`Fs` が標準ライブラリのモジュールとして振る舞うことを、本物の宣言を保ったまま確かめる。
static FAKE_FS: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{}\npub greet : Unit -> String\ngreet () = \"hi\"\n\nhidden : Unit -> String\nhidden () = \"no\"\n",
        eml_hir::STD[1].1
    )
});

fn fake_std() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Prelude.em", eml_hir::PRELUDE_SOURCE),
        ("Fs.em", FAKE_FS.as_str()),
    ]
}

/// 読んだパスを記録する読み方。読まないはずのユーザーのファイルを読んでいないことを確かめるため。
struct Recording<'a> {
    inner: MemorySource<'a>,
    read: RefCell<Vec<String>>,
}

impl<'a> Recording<'a> {
    fn new(modules: &'a [(&'a str, &'a str)]) -> Recording<'a> {
        Recording {
            inner: MemorySource(modules),
            read: RefCell::new(Vec::new()),
        }
    }

    fn read_paths(&self) -> Vec<String> {
        self.read.borrow().clone()
    }
}

impl ModuleSource for Recording<'_> {
    fn read(&self, path: &ModulePath) -> Result<String, ReadError> {
        self.read.borrow_mut().push(path.file_path());
        self.inner.read(path)
    }
}

struct LoweredStd {
    loaded: Loaded,
    map: DefMap,
    program: Program,
    /// `load_at` と同じ形の行。
    diagnostics: Vec<String>,
}

/// 標準ライブラリを差し替えて読み込み、`def_map` と `lower` まで流す。
fn lower_std(std: &[(&str, &str)], entry: &str, source: &dyn ModuleSource) -> LoweredStd {
    let (loaded, mut diagnostics) = eml_hir::load_with_std(std, ENTRY_PATH, entry, source);
    let (map, stage) = eml_hir::def_map(&loaded.modules);
    diagnostics.extend(stage);
    let (program, stage) = eml_hir::lower(&map, &loaded.modules);
    diagnostics.extend(stage);
    sort_diagnostics(&mut diagnostics);
    let diagnostics = diagnostics
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
    LoweredStd {
        loaded,
        map,
        program,
        diagnostics,
    }
}

impl LoweredStd {
    fn module(&self, name: &str) -> ModuleId {
        self.program
            .modules
            .iter()
            .find(|(_, module)| module.name == name)
            .map(|(id, _)| id)
            .expect("a loaded module")
    }

    /// `module` の中で `qualifier.name` を引いた関数を定義したモジュールの名前。
    fn qualified_in(&self, module: &str, qualifier: &str, name: &str) -> Option<&str> {
        let Resolved::Found(ValueItem::Function(id)) = self
            .map
            .resolver(self.module(module))
            .value(NameRef::Qualified { qualifier, name })
        else {
            return None;
        };
        Some(self.map.module_name(id.module))
    }
}

fn origins(loaded: &Loaded) -> Vec<ModuleOrigin> {
    loaded.modules.iter().map(|module| module.origin).collect()
}

#[test]
fn std_modules_come_after_the_entry_and_before_user_imports() {
    let (loaded, diagnostics) = eml_hir::load_with_std(
        &fake_std(),
        ENTRY_PATH,
        "import Util",
        &MemorySource(&[("Util.em", "")]),
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(names(&loaded), ["Prelude", "Main", "Std.Fs", "Util"]);
    assert_eq!(
        paths(&loaded),
        ["<std>/Prelude.em", "test.em", "<std>/Fs.em", "Util.em"]
    );
    assert_eq!(
        origins(&loaded),
        [
            ModuleOrigin::Std,
            ModuleOrigin::User,
            ModuleOrigin::Std,
            ModuleOrigin::User
        ]
    );
    assert_eq!(targets(&loaded, 1), [Some(3)]);
}

#[test]
fn a_std_module_is_reached_by_its_short_name_without_an_import() {
    let source = Recording::new(&[]);
    let lowered = lower_std(
        &fake_std(),
        "f : Unit -> String\nf () = Fs.greet ()",
        &source,
    );
    assert_eq!(lowered.diagnostics, Vec::<String>::new());
    assert_eq!(lowered.qualified_in("Main", "Fs", "greet"), Some("Std.Fs"));
    assert_eq!(names(&lowered.loaded), ["Prelude", "Main", "Std.Fs"]);
    assert_eq!(source.read_paths(), Vec::<String>::new());
    // 修飾子は1つの区切りで、正式な名前は修飾子にならない
    let lowered = lower_std(
        &fake_std(),
        "f : Unit -> String\nf () = Std.Fs.greet ()",
        &MemorySource(&[]),
    );
    assert_eq!(
        lowered.diagnostics,
        ["test.em 2:8 E1031 unknown module qualifier `Std.Fs`"]
    );
}

#[test]
fn an_import_prefers_the_user_module_of_the_same_name() {
    let entry = "import Fs\n\nf : Unit -> String\nf () = Fs.greet ()";
    let user = [("Fs.em", "pub greet : Unit -> String\ngreet () = \"user\"")];
    let lowered = lower_std(&fake_std(), entry, &MemorySource(&user));
    assert_eq!(lowered.diagnostics, Vec::<String>::new());
    assert_eq!(names(&lowered.loaded), ["Prelude", "Main", "Std.Fs", "Fs"]);
    assert_eq!(lowered.qualified_in("Main", "Fs", "greet"), Some("Fs"));
    // ユーザーのモジュールに名前がなくても、標準ライブラリへは進まない
    let lowered = lower_std(&fake_std(), entry, &MemorySource(&[("Fs.em", "")]));
    assert_eq!(
        lowered.diagnostics,
        ["test.em 4:8 E1001 cannot find value `greet` in module `Fs`"]
    );
}

#[test]
fn a_user_module_hiding_a_std_module_suggests_the_canonical_import() {
    // help は、隠れた標準ライブラリのモジュールがその名前を定義しているときだけ付ける
    let entry = "import Fs\n\nf : String -> <IO> Fs.File\nf p = Fs.open p\n\ng : Int\ng = Fs.nope";
    let lowered = eml_test_support::lower_files(entry, &[("Fs.em", "")]);
    insta::assert_snapshot!(full(&lowered.files, &lowered.diagnostics), @"
    E1002 3:20 cannot find type `File` in module `Fs`
      3:20 not found in this module
      help: the standard `Fs` is hidden by your module `Fs`; `import Std.Fs as F` reaches it
    E1001 4:7 cannot find value `open` in module `Fs`
      4:7 not found in this module
      help: the standard `Fs` is hidden by your module `Fs`; `import Std.Fs as F` reaches it
    E1001 7:5 cannot find value `nope` in module `Fs`
      7:5 not found in this module
    ");
}

#[test]
fn an_unimported_user_module_does_not_shadow_a_std_module() {
    let user = [("Fs.em", "pub greet : Unit -> String\ngreet () = \"user\"")];
    let source = Recording::new(&user);
    let lowered = lower_std(
        &fake_std(),
        "f : Unit -> String\nf () = Fs.greet ()",
        &source,
    );
    assert_eq!(lowered.diagnostics, Vec::<String>::new());
    assert_eq!(lowered.qualified_in("Main", "Fs", "greet"), Some("Std.Fs"));
    assert_eq!(names(&lowered.loaded), ["Prelude", "Main", "Std.Fs"]);
    assert_eq!(source.read_paths(), Vec::<String>::new());
}

#[test]
fn a_qualifier_means_what_each_module_imports() {
    // 入口の `import Fs` はユーザーの `Fs.em` を指すが、`Fs` を import しない `B` の `Fs` は標準ライブラリを指す
    let modules = [
        ("Fs.em", "pub greet : Unit -> String\ngreet () = \"user\""),
        ("B.em", "pub g : Unit -> String\ng () = Fs.greet ()"),
    ];
    let lowered = lower_std(&fake_std(), "import Fs\nimport B", &MemorySource(&modules));
    assert_eq!(lowered.diagnostics, Vec::<String>::new());
    assert_eq!(
        names(&lowered.loaded),
        ["Prelude", "Main", "Std.Fs", "Fs", "B"]
    );
    assert_eq!(lowered.qualified_in("Main", "Fs", "greet"), Some("Fs"));
    assert_eq!(lowered.qualified_in("B", "Fs", "greet"), Some("Std.Fs"));
}

#[test]
fn an_import_falls_back_to_the_std_module_when_the_file_is_missing() {
    let source = Recording::new(&[]);
    let (loaded, diagnostics) = eml_hir::load_with_std(
        &fake_std(),
        ENTRY_PATH,
        "import Fs\nimport Std.Fs as F",
        &source,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(names(&loaded), ["Prelude", "Main", "Std.Fs"]);
    assert_eq!(targets(&loaded, 1), [Some(2), Some(2)]);
    // `import Std.Fs` はユーザーの根を読まない
    assert_eq!(source.read_paths(), ["Fs.em"]);
}

#[test]
fn a_fallback_to_the_std_module_reads_the_user_root_once() {
    let source = Recording::new(&[("A.em", "import Fs")]);
    let (loaded, diagnostics) =
        eml_hir::load_with_std(&fake_std(), ENTRY_PATH, "import Fs\nimport A", &source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(targets(&loaded, 3), [Some(2)]);
    assert_eq!(source.read_paths(), ["Fs.em", "A.em"]);
}

#[test]
fn an_unreadable_user_file_does_not_fall_back_to_the_std_module() {
    let (loaded, diagnostics) =
        eml_hir::load_with_std(&fake_std(), ENTRY_PATH, "import Fs", &Unreadable);
    assert_eq!(
        full(&loaded.files, &diagnostics),
        "E1026 1:8 cannot read module `Fs`: stream did not contain valid UTF-8\n  1:8 `Fs.em` cannot be read\n"
    );
    assert_eq!(targets(&loaded, 1), [None]);
}

#[test]
fn the_std_root_is_reserved_for_user_imports() {
    let user = [("Std/Nope.em", ""), ("Util.em", "")];
    let source = Recording::new(&user);
    let (loaded, mut diagnostics) = eml_hir::load_with_std(
        &fake_std(),
        ENTRY_PATH,
        "import Std\nimport Std.Nope\nimport Std.Prelude\nimport Std.Prelude as P\nimport Util as Std",
        &source,
    );
    sort_diagnostics(&mut diagnostics);
    insta::assert_snapshot!(full(&loaded.files, &diagnostics), @"
    E1030 1:1 the module `Std` is reserved
      1:1 `Std` is the root of the standard library
    E1026 2:8 cannot find module `Std.Nope`
      2:8 there is no file `<std>/Nope.em`
    E1030 3:1 the module `Std.Prelude` is reserved
      3:1 the Prelude is imported implicitly
    E1030 4:1 the module `Std.Prelude` is reserved
      4:1 the Prelude is imported implicitly
    ");
    assert_eq!(targets(&loaded, 1), [None, None, None, None, Some(3)]);
    assert_eq!(source.read_paths(), ["Util.em"]);
}

#[test]
fn private_names_of_a_std_module_are_undefined() {
    // 今の Prelude の規則を、標準ライブラリのモジュールすべてに広げる (E1029 にしない)
    let lowered = lower_std(
        &fake_std(),
        "f : Unit -> String\nf () = Fs.hidden ()",
        &MemorySource(&[]),
    );
    assert_eq!(
        lowered.diagnostics,
        ["test.em 2:8 E1001 cannot find value `hidden` in module `Std.Fs`"]
    );
    let lowered = lower_std(&fake_std(), "import Std.Fs (hidden)", &MemorySource(&[]));
    assert_eq!(
        lowered.diagnostics,
        ["test.em 1:16 E1001 cannot find value `hidden` in module `Std.Fs`"]
    );
}

/// `head` の後ろに本物の `Fs` を続けたもの。診断の行を `head` の中に保ったまま、`File` などの宣言を残すため。
fn fs_after(head: &str) -> String {
    format!("{head}\n{}", eml_hir::STD[1].1)
}

#[test]
fn a_std_module_reaches_another_only_through_an_import() {
    let other = "pub x : Int\nx = 1\n";
    let without = [
        ("Prelude.em", eml_hir::PRELUDE_SOURCE),
        ("Fs.em", &fs_after("pub f : Unit -> Int\nf () = Other.x\n")),
        ("Other.em", other),
    ];
    let lowered = lower_std(&without, "", &MemorySource(&[]));
    assert_eq!(
        lowered.diagnostics,
        ["<std>/Fs.em 2:8 E1031 unknown module qualifier `Other`"]
    );
    let with = [
        ("Prelude.em", eml_hir::PRELUDE_SOURCE),
        (
            "Fs.em",
            &fs_after("import Other\n\npub f : Unit -> Int\nf () = Other.x\n"),
        ),
        ("Other.em", other),
    ];
    // 標準ライブラリのモジュールの import は、ユーザーの根を読まない
    let source = Recording::new(&[("Other.em", "")]);
    let lowered = lower_std(&with, "", &source);
    assert_eq!(lowered.diagnostics, Vec::<String>::new());
    assert_eq!(
        names(&lowered.loaded),
        ["Prelude", "Main", "Std.Fs", "Std.Other"]
    );
    assert_eq!(targets(&lowered.loaded, 2), [Some(3)]);
    assert_eq!(source.read_paths(), Vec::<String>::new());
}

#[test]
fn a_std_module_reaches_another_by_its_canonical_name_too() {
    let other = "pub x : Int\nx = 1\n";
    let std = [
        ("Prelude.em", eml_hir::PRELUDE_SOURCE),
        (
            "Fs.em",
            &fs_after("import Std.Other\n\npub f : Unit -> Int\nf () = Other.x\n"),
        ),
        ("Other.em", other),
    ];
    let source = Recording::new(&[]);
    let lowered = lower_std(&std, "", &source);
    assert_eq!(lowered.diagnostics, Vec::<String>::new());
    assert_eq!(targets(&lowered.loaded, 2), [Some(3)]);
    assert_eq!(source.read_paths(), Vec::<String>::new());
}

#[test]
fn a_std_module_cannot_import_the_prelude() {
    // 標準ライブラリのモジュールも Prelude を暗黙に取り込むので、書いた `import Prelude` は誤りである
    let std = [
        ("Prelude.em", eml_hir::PRELUDE_SOURCE),
        (
            "Fs.em",
            &fs_after("import Prelude\nimport Std.Prelude as P\n"),
        ),
    ];
    let (loaded, mut diagnostics) =
        eml_hir::load_with_std(&std, ENTRY_PATH, "", &MemorySource(&[]));
    sort_diagnostics(&mut diagnostics);
    insta::assert_snapshot!(full(&loaded.files, &diagnostics), @"
    E1030 1:1 the module `Prelude` is reserved
      1:1 the Prelude is imported implicitly
    E1030 2:1 the module `Std.Prelude` is reserved
      2:1 the Prelude is imported implicitly
    ");
    assert_eq!(targets(&loaded, 2), [None, None]);
}

#[test]
fn a_std_directory_in_the_user_root_is_never_read() {
    let user = [("std/Prelude.em", "broken ="), ("std/Fs.em", "broken =")];
    let source = Recording::new(&user);
    let (loaded, diagnostics) = eml_hir::load_with_std(
        &fake_std(),
        ENTRY_PATH,
        "import Fs\nimport Std.Fs as F",
        &source,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(names(&loaded), ["Prelude", "Main", "Std.Fs"]);
    assert_eq!(loaded.files.text(loaded.prelude), eml_hir::PRELUDE_SOURCE);
    assert_eq!(source.read_paths(), ["Fs.em"]);
}

#[test]
fn the_embedded_std_matches_the_std_directory() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    let mut on_disk = Vec::new();
    std_files(&dir, "", &mut on_disk);
    on_disk.sort();
    let mut embedded: Vec<(String, String)> = eml_hir::STD
        .iter()
        .map(|(name, text)| (name.to_string(), text.to_string()))
        .collect();
    embedded.sort();
    assert_eq!(embedded, on_disk);
    assert_eq!(eml_hir::STD[0], ("Prelude.em", eml_hir::PRELUDE_SOURCE));
}

/// `dir` の下のファイルを、`std/` からの `/` で区切ったパスと本文の組にして集める。埋め込んだ並びのパスと同じ形である。
fn std_files(dir: &Path, prefix: &str, out: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = format!("{prefix}{}", path.file_name().unwrap().to_string_lossy());
        if path.is_dir() {
            std_files(&path, &format!("{name}/"), out);
        } else {
            out.push((name, fs::read_to_string(&path).unwrap()));
        }
    }
}
