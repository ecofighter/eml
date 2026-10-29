//! `eml_rt::IoDriver` の実装 (docs/spec/vm.md の「時計と driver」)。本物の時計で待ち、compio-driver の上で
//! OS の I/O を行う `RealDriver` と、仮想の時計と仮想のネットワークで決定的に進む `TestDriver` を持つ。実行系の `Clock` が選ぶ。
//! `eml_vm` には依存しない。

mod flag;
mod mask;
mod real;
#[cfg(unix)]
mod resolve;
mod test;

pub use mask::spawn_masked;
pub use real::RealDriver;
pub use test::TestDriver;
