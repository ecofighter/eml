//! `eml_rt::IoDriver` の実装 (docs/spec/vm.md の「時計と driver」)。本物の時計で待ち、OS の I/O を行う `RealDriver` と、
//! 仮想の時計と仮想のネットワークで決定的に進む `TestDriver` を持つ。実行系の `Clock` が選ぶ。`RealDriver` は、OS を
//! 呼ばない核と OS ごとの backend からなる (docs/implementation/architecture.md の「`eml_io` の内部」)。
//! `eml_vm` には依存しない。

#![deny(unsafe_code)]

mod driver;
mod flag;
mod mask;
mod real;
mod sys;
mod test;
#[cfg(test)]
mod trust;

pub use mask::spawn_masked;
pub use real::{IoBackend, RealDriver};
pub use test::TestDriver;
