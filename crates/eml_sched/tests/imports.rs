/// 意味の核はネイティブのコードからも呼べるように、`std` と `eml_rt` だけを import する。
#[test]
fn the_task_cells_import_only_std_and_the_runtime() {
    let source = include_str!("../src/cells.rs");
    for line in source.lines() {
        let line = line.trim_start();
        if line.starts_with("use ") {
            assert!(
                line.starts_with("use std::") || line.starts_with("use eml_rt::"),
                "{line}"
            );
        }
        assert!(
            !line.contains("crate::") && !line.contains("super::"),
            "{line}"
        );
    }
}
