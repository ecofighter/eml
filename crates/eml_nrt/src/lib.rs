//! ネイティブのコードが呼ぶ実行系。エントリ、エントリのstub、致命的な誤りの道 (docs/spec/native.md の「エントリ」) と、
//! 実行の駆動 (docs/spec/native.md の「実行の始めと終わり」) を持つ。
#![deny(unsafe_code)]

mod arch;
mod c;
mod census;
mod closures;
mod constants;
mod effects;
mod enter;
mod entries;
mod heap;
pub mod image;
pub mod mode;
mod rows;
pub mod run;
mod services;
#[cfg(test)]
mod trust;
pub mod worker;

pub use arch::{JmpBuf, SUPPORTED};
#[doc(hidden)]
pub use eml_rt as __rt;
pub use eml_rt::HostRef;
pub use eml_sched::config::{Finished, RunConfig, RunStats};
pub use eml_sched::error::{RunFault, RuntimeError};
pub use enter::{
    FATAL_FAULT, FATAL_PANICKED, FATAL_RECORDED, Fatal, Raise, Thunk, detached, eml_enter,
    eml_fatal, enter,
};
pub use entries::{Class, Entry, Kind};
pub use services::{eml_nrt_failed, eml_nrt_safepoint};

// 関数の値のエントリ (docs/spec/native.md の「関数の値」)
pub use closures::eml_nrt_pap;

/// 生成したコードが呼べるすべてのエントリ。
pub fn entries<M: eml_rt::Mode>() -> impl Iterator<Item = &'static Entry> {
    [
        enter::ENTRIES::<M>(),
        services::SERVICES::<M>(),
        heap::HEAP::<M>(),
        rows::ROWS::<M>(),
        effects::EFFECTS::<M>(),
        closures::CLOSURES::<M>(),
        c::C::<M>(),
    ]
    .into_iter()
    .flat_map(|table| table.iter())
}
