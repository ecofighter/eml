//! `eml_rt::IoDriver` の実装 (docs/spec/vm.md の「時計と driver」)。本物の時計で待つ `RealDriver` と、仮想の時計で
//! 決定的に進む `TestDriver` を持つ。実行系の `Clock` が選ぶ。`eml_vm` には依存しない。

mod flag;
mod real;
mod test;

pub use real::{IDLE_TICK, RealDriver};
pub use test::TestDriver;
