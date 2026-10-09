//! item の収集 (docs/implementation/architecture.md の「`eml_hir` の内部」)。

use eml_hir::{ImportItem, ImportName, ModulePath, item_tree};
use eml_test_support::{parse, short};

fn tree(text: &str) -> (eml_hir::ItemTree, Vec<String>) {
    let parsed = parse(text);
    let (tree, mut diagnostics) = item_tree(parsed.file, &parsed.parse);
    eml_diagnostics::sort_diagnostics(&mut diagnostics);
    (tree, short(&parsed.files, &diagnostics))
}

#[test]
fn signatures_and_equations_are_grouped_by_name() {
    let (tree, diagnostics) = tree("f : Int -> Int\nf 0 = 1\nf n = n\npub g : Int\ng = 1");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let names: Vec<(&str, bool, usize)> = tree
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f.public, f.equations.len()))
        .collect();
    assert_eq!(names, [("f", false, 2), ("g", true, 1)]);
}

#[test]
fn data_effects_and_fixities_list_their_parts() {
    let (tree, _) =
        tree("pub data T = | A | B Int\neffect E where\n  get : Unit -> Int\ninfixl 6 <+>");
    assert_eq!(tree.data[0].name, "T");
    assert!(tree.data[0].public);
    let constructors: Vec<&str> = tree.data[0]
        .constructors
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(constructors, ["A", "B"]);
    assert_eq!(tree.effects[0].operations[0].name, "get");
    assert_eq!(tree.fixities[0].operators[0].0, "<+>");
}

#[test]
fn declarations_hold_what_is_known_before_resolving_names() {
    let text = "data T a b a = | A\neffect E s s where\n  get : Unit -> s\npub extern data I\ndata U\nextern f : Int\ng : Int\ng = 1";
    let (tree, diagnostics) = tree(text);
    // 型引数の重複は、2つ目以降を E1003 にして並びから除く
    assert_eq!(
        diagnostics,
        [
            "E1003 1:12 `a` is defined more than once",
            "E1003 2:12 `s` is defined more than once",
        ]
    );
    let params = |params: &[(String, eml_diagnostics::TextRange)]| -> Vec<String> {
        params
            .iter()
            .map(|(name, range)| {
                assert_eq!(&text[*range], name.as_str());
                name.clone()
            })
            .collect()
    };
    assert_eq!(params(&tree.data[0].params), ["a", "b"]);
    assert_eq!(u32::from(tree.data[0].params[0].1.start()), 7);
    assert_eq!(params(&tree.effects[0].params), ["s"]);
    let keyword = |range: Option<eml_diagnostics::TextRange>| range.map(|range| &text[range]);
    assert_eq!(keyword(tree.data[0].extern_keyword), None);
    assert_eq!(keyword(tree.data[1].extern_keyword), Some("extern"));
    assert_eq!(keyword(tree.effects[0].extern_keyword), None);
    let constructors: Vec<bool> = tree.data.iter().map(|d| d.has_constructors).collect();
    assert_eq!(constructors, [true, false, false]);
    let signatures: Vec<(&str, Option<&str>)> = tree
        .functions
        .iter()
        .map(|f| {
            let signature = f.signature.as_ref().expect("a signature");
            (
                &text[signature.name_range],
                keyword(signature.extern_keyword),
            )
        })
        .collect();
    assert_eq!(signatures, [("f", Some("extern")), ("g", None)]);
}

#[test]
fn ordering_errors_are_reported_without_resolving_names() {
    let (_, diagnostics) = tree(
        "f : Int\ng : Int\ng = 1\nf = 2\nh = 3\nk : Int\nk = 1\nk = 2\nm : Int\nm : Int\nm = 1",
    );
    assert_eq!(
        diagnostics,
        [
            "E1019 4:1 the signature of `f` is not followed by its equations",
            "E1004 5:1 `h` has no type signature",
            "E1003 10:1 `m` is defined more than once",
            "E1019 11:1 the signature of `m` is not followed by its equations",
        ]
    );
}

#[test]
fn imports_hold_their_path_qualifier_and_list() {
    let text = "import Report.Csv\nimport Report.Format as F (render, Style(..), Row, (<+>))\nimport M ((:+), x)\nimport";
    let (tree, diagnostics) = tree(text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    // 最後の `import` はパスがないので集めない (パーサが報告済み)
    assert_eq!(tree.imports.len(), 3);
    let csv = &tree.imports[0];
    assert_eq!(
        csv.path,
        ModulePath(vec!["Report".to_string(), "Csv".to_string()])
    );
    assert_eq!(csv.path.dotted(), "Report.Csv");
    assert_eq!(csv.path.file_path(), "Report/Csv.em");
    assert_eq!(csv.path.last(), "Csv");
    assert_eq!(&text[csv.path_range], "Report.Csv");
    // 別名がなければ、最後のセグメントが修飾子である
    assert_eq!(csv.qualifier, "Csv");
    assert!(csv.list.is_none());
    assert_eq!(text[csv.range].trim_end(), "import Report.Csv");
    let format = &tree.imports[1];
    assert_eq!(format.qualifier, "F");
    assert_eq!(
        import_names(text, format),
        ["value render", "type Style (..)", "type Row", "value <+>"]
    );
    // `:` で始まる演算子はパーサが E0011 にしたので集めない
    assert_eq!(import_names(text, &tree.imports[2]), ["value x"]);
}

/// 並びの名前を `value x`、`type T`、`type T (..)` の形にする。名前の位置がその名前を指すことも確かめる。
fn import_names(text: &str, import: &ImportItem) -> Vec<String> {
    import
        .list
        .as_ref()
        .expect("an import list")
        .iter()
        .map(|name| match name {
            ImportName::Value { name, range } => {
                assert_eq!(&text[*range], name.as_str());
                format!("value {name}")
            }
            ImportName::Type { name, range, all } => {
                assert_eq!(&text[*range], name.as_str());
                if *all {
                    format!("type {name} (..)")
                } else {
                    format!("type {name}")
                }
            }
        })
        .collect()
}

#[test]
fn type_class_syntax_is_lowered() {
    // 仮の E0004 はなく、導出できないクラスの E1038 だけが残る
    let lowered = eml_test_support::lower(
        "class C a where\n  m : a -> Int\n\ninstance C Int where\n  m x = x\n\nf : C a => a -> Int\nf x = 1\n\ndata D = | D deriving C",
    );
    insta::assert_snapshot!(eml_test_support::short_text(lowered.files(), &lowered.diagnostics), @"E1038 10:23 `C` cannot be derived");
}
