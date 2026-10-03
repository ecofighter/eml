//! 型付き HIR → Core IR の変換と、dup/decref の挿入パス。
//!
//! 最初の段階では仮実装で、空の `Program` を返す。中身は後の段階で TDD で実装する。

use eml_diagnostics::Diagnostic;
use eml_types::TypedModule;

/// Core IR のプログラム全体。実行時は `Arc<Program>` で読み取り専用で共有する。
#[derive(Debug, Default)]
pub struct Program {}

pub fn lower(_module: &TypedModule) -> (Program, Vec<Diagnostic>) {
    (Program::default(), Vec::new())
}
