//! 実行の方式によらない層。VMとネイティブのコードが共有する、タスクのセルの核と実行時エラーを持つ。Cranelift、`eml_lir`、
//! `eml_vm`、`eml_nrt`、`eml_native`には依存しない (docs/implementation/architecture.md の「crate の依存関係」)。

pub mod cells;
pub mod error;
