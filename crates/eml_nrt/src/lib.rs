//! ネイティブのコードが呼ぶ実行系。エントリ、エントリのstub、致命的な誤りの道を持つ (docs/spec/native.md の「エントリ」)。
#![deny(unsafe_code)]

mod arch;
mod enter;
mod entries;
#[cfg(test)]
mod trust;

pub use arch::{JmpBuf, SUPPORTED};
#[doc(hidden)]
pub use eml_rt as __rt;
pub use enter::{
    FATAL_FAULT, FATAL_PANICKED, Fatal, Raise, Thunk, detached, eml_enter, eml_fatal, enter,
};
pub use entries::{Entry, Kind};

/// エントリの表。エントリを宣言するmoduleは、自分の表をここに並べる (docs/spec/native.md の「記述子の表」)。
const TABLES: &[&[Entry]] = &[enter::ENTRIES];

/// 生成したコードが呼べるすべてのエントリ。
pub fn entries() -> impl Iterator<Item = &'static Entry> {
    TABLES.iter().flat_map(|table| table.iter())
}
