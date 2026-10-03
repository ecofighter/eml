use eml_diagnostics::{SourceFiles, has_errors};

/// 誤りのないプログラムを Core IR にして表示する。
pub fn core_text(text: &str) -> String {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, mut diagnostics) = eml_syntax::parse(file, text);
    let (module, stage) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(stage);
    let (typed, stage) = eml_types::check(&module);
    diagnostics.extend(stage);
    assert!(!has_errors(&diagnostics), "{diagnostics:#?}");
    eml_core_ir::pretty(&eml_core_ir::lower(&module, &typed))
}
