//! エフェクトの基本操作のテスト (docs/spec/native.md の「エフェクト」)。
use eml_lir::{Prim, Repr};

/// 即値の数。`evv_mask`は項目の列の長さ、`ev_new`は操作の数、`ev_set_clause`、`ev_lookup`、`ev_clause`、`root_get`、
/// `root_set`は番号である。
fn immediates(prim: &Prim) -> usize {
    match prim.name() {
        "evv_mask" | "ev_new" => 2,
        "ev_set_clause" | "ev_lookup" | "ev_clause" | "root_get" | "root_set" => 1,
        _ => 0,
    }
}

/// 呼ぶ基本操作のエントリは、記号が`eml_rt_`に基本操作の名前を続けたもので、引数は即値と行の引数を並べたものである。
/// 表を持たず、基本操作の表から導く。インラインの2つと、`empty_mut_array`、`child_abort`にはエントリがない。
#[test]
fn every_prim_entry_is_named_after_its_prim() {
    const NO_ENTRY: [&str; 4] = ["yielding", "yield_status", "empty_mut_array", "child_abort"];
    for prim in Prim::KINDS {
        let symbol = format!("eml_rt_{}", prim.name());
        let entry = eml_nrt::entries().find(|e| e.symbol == symbol);
        if NO_ENTRY.contains(&prim.name()) {
            assert!(entry.is_none(), "{symbol}");
            continue;
        }
        let entry = entry.unwrap_or_else(|| panic!("no entry {symbol}"));
        let row = prim.row();
        assert_eq!(
            entry.params.len(),
            immediates(&prim) + row.params.len(),
            "{symbol}"
        );
        assert_eq!(entry.result.is_none(), row.ret == Repr::Unit, "{symbol}");
    }
}
