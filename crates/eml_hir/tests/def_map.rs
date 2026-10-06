//! モジュールごとのスコープ表 (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 3.2〜3.4)。

use eml_hir::{Assoc, Fixity, Lookup, ValueItem};
use eml_test_support::def_map;

#[test]
fn the_first_definition_of_a_name_wins_and_later_ones_are_duplicates() {
    // 不具合2: 操作の後の同じ名前の関数が、操作を上書きしていた
    let (map, diagnostics) = def_map("effect E where\n  get : Unit -> Int\n\nget : Int\nget = 1");
    assert_eq!(diagnostics, ["E1003 4:1 `get` is defined more than once"]);
    let resolver = map.resolver(map.entry());
    assert!(matches!(
        resolver.value("get"),
        Some(ValueItem::Operation(_))
    ));
    assert!(matches!(resolver.operation("get"), Lookup::Found(_)));
}

#[test]
fn a_clause_finds_an_operation_defined_after_a_function_of_the_same_name() {
    let (map, diagnostics) = def_map("get : Int\nget = 1\n\neffect E where\n  get : Unit -> Int");
    assert_eq!(diagnostics, ["E1003 5:3 `get` is defined more than once"]);
    let resolver = map.resolver(map.entry());
    assert!(matches!(
        resolver.value("get"),
        Some(ValueItem::Function(_))
    ));
    assert!(matches!(resolver.operation("get"), Lookup::Found(_)));
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
    assert!(matches!(resolver.value("B"), Some(ValueItem::Unusable)));
    assert!(matches!(resolver.constructor("B"), Lookup::Unusable));
    assert!(matches!(resolver.operation("y"), Lookup::Unusable));
    assert!(matches!(resolver.value("x"), Some(ValueItem::Operation(_))));
}

#[test]
fn the_entry_sees_only_public_prelude_names() {
    let (map, _) = def_map("f : Int\nf = 1");
    let resolver = map.resolver(map.entry());
    assert!(resolver.value("println").is_some());
    // `negate` は `pub` でないので名前で引けない。lang item としては引ける
    assert!(resolver.value("negate").is_none());
    assert_eq!(map.lang().negate.module, map.prelude());
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
    let fixity = |precedence, assoc| Fixity { precedence, assoc };
    // ユーザーの `+` は Prelude の `+` を隠すので、Prelude の `infixl 6` は効かない
    assert_eq!(resolver.fixity("+"), fixity(9, Assoc::Left));
    assert_eq!(resolver.fixity("<+>"), fixity(2, Assoc::Right));
    assert_eq!(resolver.fixity("*"), fixity(7, Assoc::Left));
}
