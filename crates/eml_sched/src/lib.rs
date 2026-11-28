//! 実行の方式によらない層。VMとネイティブのコードが共有する、タスクのセルの核と実行時エラー、時計の選び方とdriverの作り方、doorbell、`Park`、実行の設定と結果の型 (`config.rs`) を持つ。Cranelift、`eml_lir`、
//! `eml_vm`、`eml_nrt`、`eml_native`には依存しない (docs/implementation/architecture.md の「crate の依存関係」)。

pub mod cells;
pub mod config;
pub mod core;
pub mod doorbell;
mod drive;
pub mod driver;
pub mod error;
pub mod exec;
pub mod job;
pub mod park;
pub mod policy;
mod rank;
pub mod run;
pub mod script;
pub mod sequence;

pub use eml_rt::{Control, HostRef, Limits, Stop};
