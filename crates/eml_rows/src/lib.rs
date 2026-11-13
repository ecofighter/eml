//! 組み込みの関数が実行時に何であるかを、VMとネイティブのコードのどちらにも与える層。externの行の一覧と本体 (`rows.rs`)、
//! 組み込みが持つ値を作る関数 (`constants.rs`)、`Std.Tls`と`Std.Gui`の腕 (`tls.rs`、`gui.rs`) を持つ。Cranelift、
//! `eml_lir`、`eml_vm`、`eml_nrt`、`eml_native`には依存しない (docs/implementation/architecture.mdの「crate の依存関係」)。

pub mod constants;
pub mod gui;
pub mod rows;
pub mod tls;
