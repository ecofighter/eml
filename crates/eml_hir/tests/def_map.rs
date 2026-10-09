//! モジュールごとのスコープ表 (docs/implementation/architecture.md の「`eml_hir` の内部」)。

use eml_extern::ExternType;
use eml_hir::NameRef::{self, Plain};
use eml_hir::{Assoc, DefMap, Fixity, Resolved, Silence, TypeItem, ValueItem};
use eml_test_support::{def_map, def_map_files};

fn qualified<'a>(qualifier: &'a str, name: &'a str) -> NameRef<'a> {
    NameRef::Qualified { qualifier, name }
}

/// 値が見つかれば、それを定義したモジュールの名前。
fn found_in(map: &DefMap, resolved: Resolved<ValueItem>) -> Option<&str> {
    let Resolved::Found(item) = resolved else {
        return None;
    };
    let module = match item {
        ValueItem::Function(id) => id.module,
        ValueItem::Operation(id) => id.module,
        ValueItem::Constructor(id) => id.module,
    };
    Some(map.module_name(module))
}

#[test]
fn the_first_definition_of_a_name_wins_and_later_ones_are_duplicates() {
    // 不具合2: 操作の後の同じ名前の関数が、操作を上書きしていた
    let (map, diagnostics) = def_map("effect E where\n  get : Unit -> Int\n\nget : Int\nget = 1");
    assert_eq!(diagnostics, ["E1003 4:1 `get` is defined more than once"]);
    let resolver = map.resolver(map.entry());
    assert!(matches!(
        resolver.value(Plain("get")),
        Resolved::Found(ValueItem::Operation(_))
    ));
    assert!(matches!(
        resolver.operation(Plain("get")),
        Resolved::Found(_)
    ));
}

#[test]
fn a_clause_finds_an_operation_defined_after_a_function_of_the_same_name() {
    let (map, diagnostics) = def_map("get : Int\nget = 1\n\neffect E where\n  get : Unit -> Int");
    assert_eq!(diagnostics, ["E1003 5:3 `get` is defined more than once"]);
    let resolver = map.resolver(map.entry());
    assert!(matches!(
        resolver.value(Plain("get")),
        Resolved::Found(ValueItem::Function(_))
    ));
    assert!(matches!(
        resolver.operation(Plain("get")),
        Resolved::Found(_)
    ));
}

#[test]
fn parts_of_a_duplicate_declaration_are_unusable() {
    let (map, diagnostics) = def_map(
        "data T = | A\ndata T = | B\neffect E where\n  x : Unit -> Int\neffect E where\n  y : Unit -> Int",
    );
    assert_eq!(
        diagnostics,
        [
            "E1003 2:6 `T` is defined more than once",
            "E1003 5:8 `E` is defined more than once",
        ]
    );
    let resolver = map.resolver(map.entry());
    assert_eq!(
        resolver.value(Plain("B")),
        Resolved::Silent(Silence::Unusable)
    );
    assert_eq!(
        resolver.constructor(Plain("B")),
        Resolved::Silent(Silence::Unusable)
    );
    assert_eq!(
        resolver.operation(Plain("y")),
        Resolved::Silent(Silence::Unusable)
    );
    assert!(matches!(
        resolver.value(Plain("x")),
        Resolved::Found(ValueItem::Operation(_))
    ));
}

#[test]
fn the_entry_sees_only_public_prelude_names() {
    let (map, _) = def_map("f : Int\nf = 1");
    let resolver = map.resolver(map.entry());
    assert!(matches!(
        resolver.value(Plain("println")),
        Resolved::Found(_)
    ));
    // `negate` は `pub` でないので名前で引けない。extern の索引からは引ける
    assert_eq!(resolver.value(Plain("negate")), Resolved::NotFound);
    assert_eq!(map.externs().negate.module, map.prelude());
}

#[test]
fn fixities_belong_to_the_definition_a_name_resolves_to() {
    let (map, diagnostics) = def_map(
        "(+) : Int -> Int -> Int\na + b = a\ninfixr 2 <+>\n(<+>) : Int -> Int -> Int\na <+> b = a\ninfixl 1 ++",
    );
    assert_eq!(
        diagnostics,
        ["E1022 6:10 `++` is not defined in this module"]
    );
    let resolver = map.resolver(map.entry());
    let fixity = |precedence, assoc| Some(Fixity { precedence, assoc });
    // ユーザーの `+` は Prelude の `+` を隠すので、Prelude の `infixl 6` は効かない
    assert_eq!(resolver.fixity(Plain("+")), fixity(9, Assoc::Left));
    assert_eq!(resolver.fixity(Plain("<+>")), fixity(2, Assoc::Right));
    assert_eq!(resolver.fixity(Plain("*")), fixity(7, Assoc::Left));
}

#[test]
fn prelude_fixities_follow_the_standard_table() {
    // docs/spec/declarations.md の標準の演算子の表。入口のモジュールから、Prelude の `pub` の fixity として引く
    let table: &[(&[&str], u8, Assoc)] = &[
        (&["<|"], 0, Assoc::Right),
        (&["|>"], 1, Assoc::Left),
        (&["||"], 2, Assoc::Right),
        (&["&&"], 3, Assoc::Right),
        (&["==", "!=", "<", "<=", ">", ">="], 4, Assoc::None),
        (&["++"], 5, Assoc::Right),
        (&["+", "-"], 6, Assoc::Left),
        (&["*", "/", "%"], 7, Assoc::Left),
        (&[">>", "<<"], 9, Assoc::Right),
    ];
    let (map, diagnostics) = def_map("");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let resolver = map.resolver(map.entry());
    for (ops, precedence, assoc) in table {
        for op in *ops {
            assert_eq!(
                resolver.fixity(Plain(op)),
                Some(Fixity {
                    precedence: *precedence,
                    assoc: *assoc
                }),
                "{op}"
            );
        }
    }
    // `::` は S6 のリストのコンストラクタで、まだ Prelude に定義がないので fixity も持たない
    assert_eq!(resolver.fixity(Plain("::")), Some(Fixity::DEFAULT));
}

#[test]
fn entry_definitions_shadow_prelude_names() {
    let (map, diagnostics) = def_map("not : Bool -> Bool\nnot b = b");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let resolver = map.resolver(map.entry());
    let Resolved::Found(ValueItem::Function(user)) = resolver.value(Plain("not")) else {
        panic!("`not` is a function");
    };
    assert_eq!(user.module, map.entry());
    assert_eq!(resolver.value(Plain("nope")), Resolved::NotFound);
}

#[test]
fn types_and_effects_share_the_type_namespace() {
    let (map, _) = def_map("");
    let resolver = map.resolver(map.entry());
    assert_eq!(
        resolver.type_item(Plain("Int")),
        Resolved::Found(TypeItem::Type(map.externs().ty(ExternType::Int)))
    );
    assert_eq!(
        resolver.type_item(Plain("IO")),
        Resolved::Found(TypeItem::Effect(map.externs().io))
    );
    assert_eq!(resolver.type_item(Plain("Console")), Resolved::NotFound);
}

#[test]
fn plain_names_resolve_own_then_imported_then_prelude() {
    let modules = [(
        "A.em",
        "pub x : Int\nx = 1\n\npub show_int : Int -> String\nshow_int n = \"a\"\n\npub y : Int\ny = 2",
    )];
    let (map, diagnostics) = def_map_files("import A (x, show_int, y)\n\ny : Int\ny = 3", &modules);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let resolver = map.resolver(map.entry());
    assert_eq!(found_in(&map, resolver.value(Plain("x"))), Some("A"));
    // 並びの名前は Prelude の名前を、自分の定義は並びの名前を、どちらも診断なしで隠す
    assert_eq!(found_in(&map, resolver.value(Plain("show_int"))), Some("A"));
    assert_eq!(found_in(&map, resolver.value(Plain("y"))), Some("Main"));
    assert_eq!(
        found_in(&map, resolver.value(Plain("not"))),
        Some("Prelude")
    );
}

#[test]
fn qualified_names_look_only_in_the_qualifier_modules() {
    let modules = [("A.em", "pub x : Int\nx = 1")];
    let (map, _) = def_map_files(
        "import A\nimport A as B\n\nnot : Bool -> Bool\nnot b = b",
        &modules,
    );
    let resolver = map.resolver(map.entry());
    assert_eq!(
        found_in(&map, resolver.value(qualified("A", "x"))),
        Some("A")
    );
    assert_eq!(
        found_in(&map, resolver.value(qualified("B", "x"))),
        Some("A")
    );
    // 並びのない import は修飾子だけを作る
    assert_eq!(resolver.value(Plain("x")), Resolved::NotFound);
    assert_eq!(resolver.value(qualified("A", "nope")), Resolved::NotFound);
    // 修飾した名前は修飾子のモジュールだけを引き、Prelude の名前に落ちない
    assert_eq!(resolver.value(qualified("A", "not")), Resolved::NotFound);
    // `Prelude` は暗黙の修飾子で、自分の定義で隠した名前も引ける。`pub` でない名前は定義がないものとして扱う
    assert_eq!(
        found_in(&map, resolver.value(qualified("Prelude", "not"))),
        Some("Prelude")
    );
    assert_eq!(found_in(&map, resolver.value(Plain("not"))), Some("Main"));
    assert_eq!(
        resolver.value(qualified("Prelude", "negate")),
        Resolved::NotFound
    );
    assert_eq!(
        resolver.value(qualified("C", "x")),
        Resolved::UnknownQualifier
    );
    assert_eq!(
        resolver.value(qualified("Report.A", "x")),
        Resolved::UnknownQualifier
    );
    assert_eq!(resolver.qualifier_modules("B"), ["A"]);
    assert_eq!(resolver.qualifier_modules("Prelude"), ["Prelude"]);
    assert!(resolver.qualifier_modules("C").is_empty());
}

#[test]
fn merged_qualifiers_and_imported_names_count_distinct_definitions() {
    let modules = [
        ("A.em", "pub x : Int\nx = 1\n\npub z : Int\nz = 1"),
        ("B.em", "pub y : Int\ny = 2\n\npub z : Int\nz = 2"),
    ];
    let entry = "import A as Q\nimport B as Q\nimport A (x, z)\nimport B (z)\nimport A as R (x)\n\nf : Int\nf = 1";
    let (map, diagnostics) = def_map_files(entry, &modules);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let resolver = map.resolver(map.entry());
    assert_eq!(
        found_in(&map, resolver.value(qualified("Q", "x"))),
        Some("A")
    );
    assert_eq!(
        found_in(&map, resolver.value(qualified("Q", "y"))),
        Some("B")
    );
    // 同じ定義を2つの import が出しても曖昧にしない
    assert_eq!(found_in(&map, resolver.value(Plain("x"))), Some("A"));
    let Resolved::Ambiguous(imports) = resolver.value(qualified("Q", "z")) else {
        panic!("`Q.z` is ambiguous");
    };
    assert_eq!(imports.len(), 2);
    let Resolved::Ambiguous(imports) = resolver.value(Plain("z")) else {
        panic!("`z` is ambiguous");
    };
    assert_eq!(imports.len(), 2);
    assert!(imports[0].start() < imports[1].start());
    assert_eq!(resolver.qualifier_modules("Q"), ["A", "B"]);
}

#[test]
fn kind_filtered_positions_count_only_their_kind() {
    let modules = [
        ("A.em", "pub get : Unit -> Int\nget () = 1"),
        (
            "B.em",
            "pub effect E where\n  get : Unit -> Int\n\npub data T = | Mk Int",
        ),
    ];
    let (map, diagnostics) = def_map_files(
        "import A (get)\nimport B (E(..), T(..))\n\nf : Int\nf = 1",
        &modules,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let resolver = map.resolver(map.entry());
    assert!(matches!(
        resolver.value(Plain("get")),
        Resolved::Ambiguous(_)
    ));
    // handler の節の先頭は操作だけを見るので、関数の `get` と競合しない
    assert!(matches!(
        resolver.operation(Plain("get")),
        Resolved::Found(_)
    ));
    assert!(matches!(
        resolver.constructor(Plain("Mk")),
        Resolved::Found(_)
    ));
    assert!(matches!(
        resolver.type_item(Plain("E")),
        Resolved::Found(TypeItem::Effect(_))
    ));
    assert!(matches!(
        resolver.type_item(Plain("T")),
        Resolved::Found(TypeItem::Type(_))
    ));
}

#[test]
fn an_uppercase_name_without_dots_imports_only_the_type() {
    let modules = [(
        "B.em",
        "pub data T = | Mk Int\n\npub effect E where\n  get : Unit -> Int",
    )];
    let (map, _) = def_map_files("import B (T, E)\n\nf : Int\nf = 1", &modules);
    let resolver = map.resolver(map.entry());
    assert!(matches!(resolver.type_item(Plain("T")), Resolved::Found(_)));
    assert_eq!(resolver.constructor(Plain("Mk")), Resolved::NotFound);
    assert_eq!(resolver.operation(Plain("get")), Resolved::NotFound);
}

#[test]
fn import_lists_report_missing_and_private_names_once() {
    let modules = [(
        "A.em",
        "pub x : Int\nx = 1\n\nsecret : Int\nsecret = 2\n\ndata Hidden = | H\n\npub data Shown = | S",
    )];
    let (map, diagnostics) = def_map_files(
        "import A (x, nope, secret, Nope, Hidden(..), Shown, S)\n\nf : Int\nf = 1",
        &modules,
    );
    assert_eq!(
        diagnostics,
        [
            "E1001 1:14 cannot find value `nope` in module `A`",
            "E1029 1:20 `secret` is not public",
            "E1002 1:28 cannot find type or effect `Nope` in module `A`",
            "E1029 1:34 `Hidden` is not public",
            "E1002 1:53 cannot find type or effect `S` in module `A`",
        ]
    );
    let resolver = map.resolver(map.entry());
    assert_eq!(found_in(&map, resolver.value(Plain("x"))), Some("A"));
    // 並びで報告した名前は、本体で使っても診断を重ねない
    assert_eq!(
        resolver.value(Plain("nope")),
        Resolved::Silent(Silence::Broken)
    );
    assert_eq!(
        resolver.value(Plain("secret")),
        Resolved::Silent(Silence::Broken)
    );
    assert_eq!(
        resolver.type_item(Plain("Nope")),
        Resolved::Silent(Silence::Broken)
    );
    assert_eq!(
        resolver.type_item(Plain("Hidden")),
        Resolved::Silent(Silence::Broken)
    );
    assert_eq!(
        resolver.constructor(Plain("H")),
        Resolved::Silent(Silence::Broken)
    );
    assert!(matches!(
        resolver.type_item(Plain("Shown")),
        Resolved::Found(TypeItem::Type(_))
    ));
    // `Shown` だけでは、コンストラクタを取り込まない
    assert_eq!(resolver.constructor(Plain("S")), Resolved::NotFound);
}

#[test]
fn broken_imports_answer_unknown_without_diagnostics() {
    let modules = [("A.em", "pub x : Int\nx = 1")];
    let (map, _) = def_map_files(
        "import Missing (show_int)\nimport Missing as M\nimport A as M\n\nf : Int\nf = 1",
        &modules,
    );
    let resolver = map.resolver(map.entry());
    // 壊れた import の並びの名前も、Prelude の名前を隠す
    assert_eq!(
        resolver.value(Plain("show_int")),
        Resolved::Silent(Silence::Broken)
    );
    // 合流した修飾子は、壊れていないモジュールの定義がちょうど1つ見つかれば、それを使う
    assert_eq!(
        found_in(&map, resolver.value(qualified("M", "x"))),
        Some("A")
    );
    assert_eq!(
        resolver.value(qualified("M", "y")),
        Resolved::Silent(Silence::Broken)
    );
    assert_eq!(resolver.qualifier_modules("M"), ["A"]);
}

#[test]
fn unusable_operators_take_the_default_fixity() {
    // 重複した宣言の部品は E1003 で報告済みなので、組み直しは既定の fixity で続ける。壊れた import の演算子とは違い、
    // 列を誤りにしない
    let (map, _) = def_map("data T = | A\ndata T = | Int :+ Int");
    let resolver = map.resolver(map.entry());
    let resolved = resolver.value(Plain(":+"));
    assert_eq!(resolved, Resolved::Silent(Silence::Unusable));
    assert_eq!(resolver.fixity_of(&resolved), Some(Fixity::DEFAULT));
}

#[test]
fn imported_operators_carry_public_fixities() {
    let modules = [
        (
            "A.em",
            "pub infixr 2 <+>\npub (<+>) : Int -> Int -> Int\na <+> b = a\n\ninfixr 3 <^>\npub (<^>) : Int -> Int -> Int\na <^> b = a\n\npub (<%>) : Int -> Int -> Int\na <%> b = a",
        ),
        ("B.em", "pub (<%>) : Int -> Int -> Int\na <%> b = a"),
    ];
    let entry = "import A ((<+>), (<^>), (<%>))\nimport B ((<%>))\nimport Missing ((<*>))\n\nf : Int\nf = 1";
    let (map, _) = def_map_files(entry, &modules);
    let resolver = map.resolver(map.entry());
    assert_eq!(
        resolver.fixity(Plain("<+>")),
        Some(Fixity {
            precedence: 2,
            assoc: Assoc::Right
        })
    );
    // `pub` でない fixity の宣言は、別のモジュールでは効かない
    assert_eq!(resolver.fixity(Plain("<^>")), Some(Fixity::DEFAULT));
    // 曖昧な演算子と壊れた import から来た演算子は、fixity が決まらない
    assert_eq!(resolver.fixity(Plain("<%>")), None);
    assert_eq!(resolver.fixity(Plain("<*>")), None);
}

#[test]
fn an_import_qualified_as_prelude_does_not_merge_with_the_implicit_qualifier() {
    // 修飾子が `Prelude` になる import は E1030 で、暗黙の修飾子 `Prelude` と合流させない (docs/spec/modules.md の「Prelude」)
    let modules = [("A.em", "pub x : Int\nx = 1")];
    let (map, _) = def_map_files(
        "import Prelude\nimport Util.Prelude\nimport A as Prelude\n\nnot : Bool -> Bool\nnot b = b",
        &modules,
    );
    let resolver = map.resolver(map.entry());
    assert_eq!(
        found_in(&map, resolver.value(qualified("Prelude", "not"))),
        Some("Prelude")
    );
    assert_eq!(
        resolver.value(qualified("Prelude", "nope")),
        Resolved::NotFound
    );
    assert_eq!(
        resolver.value(qualified("Prelude", "x")),
        Resolved::NotFound
    );
    assert_eq!(resolver.qualifier_modules("Prelude"), ["Prelude"]);
}

/// Prelude 以外の型、エフェクト、コンストラクタを `種類 モジュール.名前 -> 表示名` の行にする。
fn shown_names(lowered: &eml_test_support::Lowered) -> Vec<String> {
    let program = &lowered.program;
    let names = &program.names;
    let module = |id: eml_hir::ModuleId| program.modules[id].name.clone();
    let mut out = Vec::new();
    for (id, def) in program
        .types()
        .filter(|(id, _)| id.module != program.prelude)
    {
        out.push(format!(
            "type {}.{} -> {}",
            module(id.module),
            def.name,
            names.ty(id)
        ));
    }
    for (id, def) in program
        .effects()
        .filter(|(id, _)| id.module != program.prelude)
    {
        out.push(format!(
            "effect {}.{} -> {}",
            module(id.module),
            def.name,
            names.effect(id)
        ));
    }
    for (id, def) in program
        .constructors()
        .filter(|(id, _)| id.module != program.prelude)
    {
        out.push(format!(
            "constructor {}.{} -> {}",
            module(id.module),
            def.name,
            names.constructor(id)
        ));
    }
    out
}

#[test]
fn names_defined_by_two_modules_are_qualified() {
    let entry = "import Report\n\ndata Bool = | Yes\n\ndata Cont = | C\n\neffect Log where\n  log : String -> Unit\n\neffect Row where\n  row : Unit -> Unit";
    let report = "pub data Row = | Row Int\n\npub effect Log where\n  note : String -> Unit\n\npub data Answer = | Yes | No";
    let lowered = eml_test_support::lower_files(entry, &[("Report.em", report)]);
    assert_eq!(
        eml_test_support::short(lowered.files(), &lowered.diagnostics),
        Vec::<String>::new()
    );
    // 型とエフェクトは合わせて数え、コンストラクタは別に数える。継続は普通の関数型なので、`Cont` はユーザーの定義だけである
    assert_eq!(
        shown_names(&lowered),
        [
            "type Main.Bool -> Main.Bool",
            "type Main.Cont -> Cont",
            "type Std.Fs.File -> File",
            "type Report.Row -> Report.Row",
            "type Report.Answer -> Answer",
            "effect Main.Log -> Main.Log",
            "effect Main.Row -> Main.Row",
            "effect Report.Log -> Report.Log",
            "constructor Main.Yes -> Main.Yes",
            "constructor Main.C -> C",
            "constructor Report.Row -> Row",
            "constructor Report.Yes -> Report.Yes",
            "constructor Report.No -> No",
        ]
    );
    let program = &lowered.program;
    let names = &program.names;
    assert_eq!(names.ty(program.lang.bool), "Prelude.Bool");
    assert_eq!(names.ty(program.extern_type(ExternType::Int)), "Int");
    assert_eq!(names.effect(program.io()), "IO");
    assert_eq!(names.constructor(program.lang.true_ctor), "True");
    assert_eq!(names.unit(), "Unit");
}

#[test]
fn a_duplicate_in_one_module_counts_once() {
    let lowered = eml_test_support::lower("data T = | A\n\ndata T = | B");
    assert_eq!(
        shown_names(&lowered),
        [
            "type Main.T -> T",
            "type Main.T -> T",
            "type Std.Fs.File -> File",
            "constructor Main.A -> A",
            "constructor Main.B -> B",
        ]
    );
}

#[test]
fn a_user_unit_qualifies_the_empty_record() {
    let lowered = eml_test_support::lower("data Unit = | U");
    assert_eq!(
        shown_names(&lowered),
        [
            "type Main.Unit -> Main.Unit",
            "type Std.Fs.File -> File",
            "constructor Main.U -> U"
        ]
    );
    assert_eq!(lowered.program.names.unit(), "Prelude.Unit");
}

#[test]
fn a_std_name_defined_by_a_user_module_is_qualified_by_the_canonical_name() {
    let fs = format!(
        "{}\npub data Tag = | A\n\npub data Only = | O",
        eml_hir::STD[1].1
    );
    let std = [("Prelude.em", eml_hir::PRELUDE_SOURCE), ("Fs.em", &fs)];
    let lowered = eml_test_support::lower_with_std(&std, "data Tag = | B");
    assert_eq!(
        eml_test_support::short(lowered.files(), &lowered.diagnostics),
        Vec::<String>::new()
    );
    assert_eq!(
        shown_names(&lowered),
        [
            "type Main.Tag -> Main.Tag",
            "type Std.Fs.File -> File",
            "type Std.Fs.Tag -> Std.Fs.Tag",
            "type Std.Fs.Only -> Only",
            "constructor Main.B -> B",
            "constructor Std.Fs.A -> A",
            "constructor Std.Fs.O -> O",
        ]
    );
}

/// `M0` が `M1` を、`M1` が `M2` を import する、`self.0` 個のモジュールの鎖。
struct Chain(usize);

impl eml_hir::ModuleSource for Chain {
    fn read(&self, path: &eml_hir::ModulePath) -> Result<String, eml_hir::ReadError> {
        let index: usize = path.dotted()[1..].parse().expect("a module of the chain");
        Ok(if index + 1 < self.0 {
            format!("import M{}\n", index + 1)
        } else {
            String::new()
        })
    }
}

#[test]
fn a_long_import_chain_does_not_overflow_the_stack() {
    // 循環の検査を再帰で深さ優先にたどると、鎖の長さだけスタックを使う
    let (loaded, diagnostics) = eml_hir::load("test.em", "import M0\n", &Chain(100_000));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let (_, diagnostics) = eml_hir::def_map(&loaded.modules);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
