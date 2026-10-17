//! REPL のセッションの読み込みと名前解決 (docs/spec/modules.md の「REPL のセッション」)。

use eml_diagnostics::sort_diagnostics;
use eml_hir::{
    Assoc, ConstructorId, DefMap, Fixity, FunctionKind, ImportCache, ImportTarget, InputForm,
    InputKind, Loaded, ModuleId, ModuleOrigin, ModulePath, NameRef, Program, Resolved,
    SessionInput, TypeDefId,
};
use eml_test_support::{MemorySource, full_with_paths, located};
use la_arena::{Idx, RawIdx};

/// 入力は (入力の番号, テキスト)。
fn inputs<'a>(texts: &[(u32, &'a str)]) -> Vec<SessionInput<'a>> {
    texts
        .iter()
        .map(|&(number, text)| SessionInput {
            number,
            text,
            form: InputForm::Typed,
        })
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
    /// ラベル、note、help まで、位置に表示のパスを付けた診断。
    report: String,
}

fn lower(texts: &[(u32, &str)], modules: &[(&str, &str)]) -> Lowered {
    let (loaded, mut diagnostics, _) = eml_hir::load_session(
        eml_hir::STD,
        &inputs(texts),
        "",
        "",
        &MemorySource(modules),
        &ImportCache::default(),
    );
    let (def_map, stage) = eml_hir::def_map(&loaded.modules);
    diagnostics.extend(stage);
    let (program, stage) = eml_hir::lower(&def_map, &loaded.modules);
    diagnostics.extend(stage);
    sort_diagnostics(&mut diagnostics);
    let report = full_with_paths(&loaded.files, &diagnostics);
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
        report,
    }
}

impl Lowered {
    fn module(&self, name: &str) -> ModuleId {
        module_named(&self.loaded, name)
    }

    /// `module` で修飾しない値の名前を引き、見つかった item を定義したモジュールの名前を返す。
    fn value_in(&self, module: &str, name: &str) -> Option<&str> {
        let Resolved::Found(item) = self
            .def_map
            .resolver(self.module(module))
            .value(NameRef::Plain(name))
        else {
            return None;
        };
        Some(self.def_map.module_name(item.module()))
    }

    /// `module` の `index` 番の型の表示名。
    fn type_name(&self, module: &str, index: u32) -> &str {
        let id = TypeDefId::new(self.module(module), Idx::from_raw(RawIdx::from(index)));
        self.program.names.ty(id)
    }

    fn constructor_name(&self, module: &str, index: u32) -> &str {
        let id = ConstructorId::new(self.module(module), Idx::from_raw(RawIdx::from(index)));
        self.program.names.constructor(id)
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
    assert_eq!(names(&loaded), ["Prelude", "<input 1>", "<input 3>"]);
    assert_eq!(
        origins(&loaded),
        [
            ModuleOrigin::Std,
            ModuleOrigin::Session(1),
            ModuleOrigin::Session(3),
        ]
    );
    // 今の入力 (最後の入力) が入口である
    assert_eq!(loaded.files.path(loaded.entry), "<input 3>");
}

#[test]
fn imports_of_inputs_follow_the_inputs_in_the_order_they_are_found() {
    // 前の入力の修飾子も標準ライブラリのモジュールを読む (docs/spec/modules.md の「REPL のセッション」)
    let (loaded, diagnostics, _) = load(
        &[
            (1, "import A\n\nf : String -> <IO> Fs.File\nf p = Fs.open p"),
            (2, "import B"),
        ],
        &[("A.em", ""), ("B.em", "")],
        &ImportCache::default(),
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(
        names(&loaded),
        ["Prelude", "<input 1>", "<input 2>", "A", "Std.Fs", "B"]
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

const A_AND_B: &[(&str, &str)] = &[
    ("A.em", "pub x : Int\nx = 1"),
    ("B.em", "pub x : Int\nx = 2"),
];

#[test]
fn later_inputs_see_earlier_definitions_without_pub() {
    let lowered = lower(&[(1, "f : Int\nf = 1"), (2, "g : Int\ng = f")], &[]);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    assert_eq!(lowered.value_in("<input 2>", "f"), Some("<input 1>"));
}

#[test]
fn the_newest_version_wins_and_is_not_a_duplicate() {
    let lowered = lower(
        &[
            (1, "f : Int\nf = 1"),
            (2, "f : Int\nf = 2"),
            (3, "g : Int\ng = f"),
        ],
        &[],
    );
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    assert_eq!(lowered.value_in("<input 3>", "f"), Some("<input 2>"));
    // 1つの入力の中の重複は、今と同じく E1003 である
    let lowered = lower(&[(1, "data T = | A\ndata T = | B")], &[]);
    assert_eq!(
        lowered.diagnostics,
        ["<input 1> 2:6 E1003 `T` is defined more than once"]
    );
}

#[test]
fn earlier_inputs_do_not_see_later_ones() {
    let lowered = lower(&[(1, "g : Int\ng = h"), (2, "h : Int\nh = 1")], &[]);
    assert_eq!(
        lowered.diagnostics,
        ["<input 1> 2:5 E1001 cannot find value `h`"]
    );
}

#[test]
fn the_newest_definer_stops_the_lookup_whatever_its_kind() {
    // 操作 `ask` を関数に定義し直した後の handler の節は、古い操作を見つけない
    let lowered = lower(
        &[
            (1, "effect Ask where\n  ask : Unit -> Int"),
            (2, "ask : Int -> Int\nask x = x"),
            (
                3,
                "f : Unit -> Int\nf () = handle 1 with\n  | ctl ask () k -> k 1",
            ),
        ],
        &[],
    );
    insta::assert_snapshot!(lowered.diagnostics.join("\n"), @"<input 3> 3:9 E1001 cannot find effect operation `ask`");
    // クラス `C` を型に定義し直した後の instance の頭は、古いクラスを見つけない
    let lowered = lower(
        &[
            (1, "class C a where\n  m : a -> Int"),
            (2, "data C = | C1"),
            (3, "instance C Int where\n  m _ = 1"),
        ],
        &[],
    );
    insta::assert_snapshot!(lowered.diagnostics.join("\n"), @"<input 3> 1:10 E1041 `C` is not a class");
}

#[test]
fn imports_are_looked_up_per_input_newest_first() {
    let lowered = lower(
        &[
            (1, "import A (x)"),
            (2, "import B (x)"),
            (3, "y : Int\ny = x"),
        ],
        A_AND_B,
    );
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    assert_eq!(lowered.value_in("<input 3>", "x"), Some("B"));
}

#[test]
fn an_ambiguity_is_counted_within_one_input_and_points_into_its_file() {
    let lowered = lower(
        &[(1, "import A (x)\nimport B (x)"), (2, "y : Int\ny = x")],
        A_AND_B,
    );
    insta::assert_snapshot!(lowered.report, @"
    E1028 <input 2> 2:5 `x` is ambiguous
      <input 2> 2:5 this name refers to more than one definition
      <input 1> 1:1 one of the definitions is imported here
      <input 1> 2:1 one of the definitions is imported here
    ");
}

#[test]
fn session_definitions_hide_imports_whatever_the_order() {
    let lowered = lower(
        &[
            (1, "x : Int\nx = 0"),
            (2, "import A (x)"),
            (3, "y : Int\ny = x"),
        ],
        A_AND_B,
    );
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    assert_eq!(lowered.value_in("<input 3>", "x"), Some("<input 1>"));
}

#[test]
fn qualifiers_come_from_the_imports_of_earlier_inputs() {
    let lowered = lower(
        &[
            (1, "import A"),
            (2, "import B as Q"),
            (3, "y : Int\ny = A.x\n\nz : Int\nz = Q.x"),
        ],
        A_AND_B,
    );
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
}

#[test]
fn fixities_apply_across_inputs_without_pub() {
    let lowered = lower(
        &[
            (1, "infixl 6 <+>\n(<+>) : Int -> Int -> Int\na <+> b = a"),
            (2, "y : Int\ny = 1"),
        ],
        &[],
    );
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let fixity = lowered
        .def_map
        .resolver(lowered.module("<input 2>"))
        .fixity(NameRef::Plain("<+>"));
    assert_eq!(
        fixity,
        Some(Fixity {
            precedence: 6,
            assoc: Assoc::Left
        })
    );
}

#[test]
fn a_fixity_needs_its_operator_in_the_same_input() {
    let lowered = lower(
        &[
            (1, "(<+>) : Int -> Int -> Int\na <+> b = a"),
            (2, "infixl 6 <+>"),
        ],
        &[],
    );
    assert_eq!(
        lowered.diagnostics,
        ["<input 2> 1:10 E1022 `<+>` is not defined in this input"]
    );
}

#[test]
fn the_orphan_rule_counts_the_session_as_one_module() {
    let lowered = lower(
        &[
            (1, "class C a where\n  m : a -> Int"),
            (2, "data T = | T"),
            (3, "instance C T where\n  m _ = 1"),
        ],
        &[],
    );
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    // Prelude のクラスと Prelude の型の instance は、セッションでも orphan である
    let lowered = lower(&[(1, "instance Show Int where\n  show _ = \"\"")], &[]);
    insta::assert_snapshot!(lowered.diagnostics.join("\n"), @"<input 1> 1:15 E1034 an instance of `Show` for `Int` must be in the module of `Show` or of `Int`");
}

#[test]
fn an_instance_is_not_redefined_in_a_later_input() {
    let lowered = lower(
        &[
            (1, "data T = | T deriving (Show)"),
            (2, "instance Show T where\n  show _ = \"t\""),
        ],
        &[],
    );
    insta::assert_snapshot!(lowered.diagnostics.join("\n"), @"<input 2> 1:15 E1035 `T` already has an instance of `Show`");
}

#[test]
fn public_items_may_use_private_types_in_a_session() {
    let lowered = lower(&[(1, "data P = | P\npub f : P -> Int\nf _ = 1")], &[]);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
}

#[test]
fn old_versions_are_shown_with_their_input_number() {
    let lowered = lower(
        &[
            (1, "data T = | A"),
            (2, "data T = | B"),
            (3, "data U = | A"),
        ],
        &[],
    );
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    assert_eq!(lowered.type_name("<input 1>", 0), "T#1");
    assert_eq!(lowered.type_name("<input 2>", 0), "T");
    assert_eq!(lowered.constructor_name("<input 1>", 0), "A#1");
    assert_eq!(lowered.constructor_name("<input 3>", 0), "A");
}

#[test]
fn a_session_name_is_unqualified_and_the_other_module_is_qualified() {
    let lowered = lower(&[(1, "data Bool = | Yes")], &[]);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    assert_eq!(lowered.type_name("<input 1>", 0), "Bool");
    let prelude_bool = lowered.program.lang.bool;
    assert_eq!(lowered.program.names.ty(prelude_bool), "Prelude.Bool");
}

#[test]
fn an_expression_input_becomes_an_input_function() {
    let lowered = lower(&[(1, "1 + 2")], &[]);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let input = lowered
        .program
        .input()
        .expect("the entry has an input function");
    assert_eq!(input.module, lowered.module("<input 1>"));
    let function = &lowered.program[input];
    assert_eq!(function.kind, FunctionKind::Input(InputKind::Expr));
    assert!(function.signature.is_none());
    insta::assert_snapshot!(eml_hir::pretty(&lowered.program), @"
    input$1 : <no signature>
    input$1 () = (+ 1 2)
    ");
}

#[test]
fn a_let_input_binds_a_name_to_its_root() {
    let lowered = lower(&[(1, "let x = 1 + 2")], &[]);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let input = lowered
        .program
        .input()
        .expect("the entry has an input function");
    let FunctionKind::Input(InputKind::Let(binding)) = lowered.program[input].kind else {
        panic!("a let input");
    };
    let binding = &lowered.program[binding];
    assert_eq!(
        (binding.name.as_str(), binding.root, binding.input),
        ("x", 1, input)
    );
    insta::assert_snapshot!(eml_hir::pretty(&lowered.program), @"
    input$1 : <no signature>
    input$1 () = (+ 1 2)
    let x = root 1 <- input$1
    ");
}

#[test]
fn a_let_input_keeps_its_type_annotation() {
    let lowered = lower(&[(1, "let xs : List Int = []")], &[]);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    insta::assert_snapshot!(eml_hir::pretty(&lowered.program), @"
    input$1 : <no signature>
    input$1 () = ([] : List Int)
    let xs = root 1 <- input$1
    ");
}

#[test]
fn later_inputs_and_functions_refer_to_a_binding() {
    let lowered = lower(
        &[
            (1, "let n = 3"),
            (2, "f : Int -> Int\nf x = x + n"),
            (3, "f n"),
        ],
        &[],
    );
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    assert_eq!(lowered.value_in("<input 3>", "n"), Some("<input 1>"));
    insta::assert_snapshot!(eml_hir::pretty(&lowered.program), @"
    -- <input 1>
    input$1 : <no signature>
    input$1 () = 3
    let n = root 1 <- input$1
    -- <input 2>
    f : Int -> Int
    f x#0 = (+ x#0 @<input 1>.n)
    -- <input 3>
    input$3 : <no signature>
    input$3 () = (@<input 2>.f @<input 1>.n)
    ");
}

#[test]
fn a_binding_does_not_see_itself() {
    // 右辺の `x` は前の版の `x` である
    let lowered = lower(&[(1, "let x = 1"), (2, "let x = x + 1")], &[]);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    insta::assert_snapshot!(eml_hir::pretty(&lowered.program), @"
    -- <input 1>
    input$1 : <no signature>
    input$1 () = 1
    let x = root 1 <- input$1
    -- <input 2>
    input$2 : <no signature>
    input$2 () = (+ @<input 1>.x 1)
    let x = root 2 <- input$2
    ");
    let lowered = lower(&[(1, "let y = y")], &[]);
    assert_eq!(
        lowered.diagnostics,
        ["<input 1> 1:9 E1001 cannot find value `y`"]
    );
}

/// 名前1つでない `let` の入力はパーサが E0011 で報告する。HIR は診断を重ねない。
#[test]
fn a_let_input_binds_one_name() {
    let lowered = lower(&[(1, "let (a, b) = (1, 2)")], &[]);
    insta::assert_snapshot!(lowered.report, @"
    E0011 <input 1> 1:5 a `let` input binds one name
      <input 1> 1:5 write `let name = …` and take the value apart in a later input
    ");
}

#[test]
fn a_file_has_no_input_function() {
    let lowered = eml_test_support::lower("main : Unit -> Unit\nmain () = ()");
    assert_eq!(lowered.program.input(), None);
}

/// 読み方を選んだ入力でセッションを読み、def_map と lower を流す。診断は `パス 行:列 番号 メッセージ` の行にする。
fn lower_forms(
    inputs: &[SessionInput],
    root: &str,
    entry_file: &str,
    modules: &[(&str, &str)],
) -> (Loaded, Program, Vec<String>) {
    let (loaded, mut diagnostics, _) = eml_hir::load_session(
        eml_hir::STD,
        inputs,
        root,
        entry_file,
        &MemorySource(modules),
        &ImportCache::default(),
    );
    let (def_map, found) = eml_hir::def_map(&loaded.modules);
    diagnostics.extend(found);
    let (program, found) = eml_hir::lower(&def_map, &loaded.modules);
    diagnostics.extend(found);
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
    (loaded, program, lines)
}

/// `:load` のファイルはファイルの文法で読み、表示のパスはファイルのパスである。モジュールの名前は `<input 1>` のまま
/// (docs/spec/modules.md の「REPL のセッション」)。
#[test]
fn a_file_input_is_read_as_a_file_under_its_path() {
    let file = "pub inc : Int -> Int\ninc x = x + 1\n\nhidden : Int\nhidden = 2\n";
    let inputs = [
        SessionInput {
            number: 1,
            text: file,
            form: InputForm::File {
                path: "app/main.em",
            },
        },
        SessionInput {
            number: 2,
            text: "inc hidden",
            form: InputForm::Typed,
        },
    ];
    let (loaded, program, diagnostics) = lower_forms(&inputs, "app/", "main.em", &[]);
    assert_eq!(diagnostics, Vec::<String>::new());
    let module = module_named(&loaded, "<input 1>");
    assert_eq!(loaded.files.path(program.file(module)), "app/main.em");
    assert_eq!(
        loaded.modules[index(module)].origin,
        ModuleOrigin::Session(1)
    );
}

/// 式だけのテキストは、入力の入口なら式の入力になる。ファイルの文法では構文の誤りで、入力の関数を作らない。
#[test]
fn a_file_input_does_not_take_an_expression() {
    let inputs = [SessionInput {
        number: 1,
        text: "1 + 2",
        form: InputForm::File { path: "main.em" },
    }];
    let (_, program, diagnostics) = lower_forms(&inputs, "", "main.em", &[]);
    assert!(
        diagnostics.contains(&"main.em 1:6 E0011 expected `=`".to_string()),
        "{diagnostics:?}"
    );
    assert_eq!(program.input(), None);
}

/// ファイルは `eml run` の入口と同じく import できない。依存先からも、後の入力からも E1030 である。
#[test]
fn the_loaded_file_cannot_be_imported() {
    let file = "import Util\nmain : Unit -> <IO> Unit\nmain _ = Util.go ()\n";
    let util = "import Main2\npub go : Unit -> <IO> Unit\ngo _ = println \"go\"\n";
    let inputs = [SessionInput {
        number: 1,
        text: file,
        form: InputForm::File { path: "Main2.em" },
    }];
    let (_, _, diagnostics) = lower_forms(&inputs, "", "Main2.em", &[("Util.em", util)]);
    assert!(
        diagnostics.iter().any(|d| d.contains("E1030")),
        "{diagnostics:?}"
    );
    // ファイルを受け付けなかったセッションでも、入口のファイル名は渡るので同じ
    let inputs = [SessionInput {
        number: 2,
        text: "import Main2",
        form: InputForm::Typed,
    }];
    let (_, _, diagnostics) = lower_forms(&inputs, "", "Main2.em", &[("Main2.em", file)]);
    assert!(
        diagnostics.iter().any(|d| d.contains("E1030")),
        "{diagnostics:?}"
    );
}

#[test]
fn a_query_becomes_a_query_input_function() {
    let inputs = [
        SessionInput {
            number: 1,
            text: "let n = 1",
            form: InputForm::Typed,
        },
        SessionInput {
            number: 2,
            text: "n + 1",
            form: InputForm::Query,
        },
    ];
    let (_, program, diagnostics) = lower_forms(&inputs, "", "", &[]);
    assert_eq!(diagnostics, Vec::<String>::new());
    let input = program.input().expect("the query has an input function");
    assert_eq!(program[input].kind, FunctionKind::Input(InputKind::Query));
}
