//! ネイティブのコードが呼ぶ実行系。エントリ、エントリのstub、致命的な誤りの道 (docs/spec/native.md の「エントリ」) と、
//! 実行の駆動 (docs/spec/native.md の「実行の始めと終わり」) を持つ。
#![deny(unsafe_code)]

mod arch;
mod census;
mod constants;
mod enter;
mod entries;
pub mod image;
pub mod run;
mod services;
#[cfg(test)]
mod trust;
pub mod worker;

pub use arch::{JmpBuf, SUPPORTED};
#[doc(hidden)]
pub use eml_rt as __rt;
pub use eml_rt::HostRef;
pub use eml_sched::config::{Finished, RunConfig};
pub use eml_sched::error::{RunFault, RuntimeError};
pub use enter::{
    FATAL_FAULT, FATAL_PANICKED, FATAL_RECORDED, Fatal, Raise, Thunk, detached, eml_enter,
    eml_fatal, enter,
};
pub use entries::{Entry, Kind};
pub use services::{eml_nrt_failed, eml_nrt_safepoint};

/// エントリの表。エントリを宣言するmoduleは、自分の表をここに並べる (docs/spec/native.md の「記述子の表」)。
const TABLES: &[&[Entry]] = &[enter::ENTRIES, services::SERVICES];

/// 生成したコードが呼べるすべてのエントリ。
pub fn entries() -> impl Iterator<Item = &'static Entry> {
    TABLES.iter().flat_map(|table| table.iter())
}
