//! 時計の選び方 `Clock` と、時刻のミリ秒への丸め (docs/spec/vm.md の「時計と driver」)。driver そのものは `eml_io` にある。

use std::io;
use std::time::Duration;

use eml_io::{RealDriver, TestDriver};
use eml_rt::{Control, IoDriver};

/// タイマーが使う時計の選び方。`RunConfig::with_clock` と `Machine::with_clock` で選ぶ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Clock {
    /// 単調な本物の時計で眠る本物の driver。
    Real,
    /// すべてのタスクが待つときにだけ進むテストの driver。
    #[default]
    Virtual,
}

impl Clock {
    /// この時計の新しい driver。`seed` は `Sched::seed` で、テストの driver が決着の組を混ぜるのに使う。driver は
    /// 作ったスレッドの上でだけ使うので、run を行うスレッドで作る (docs/spec/vm.md の「時計と driver」)。
    pub(crate) fn driver(self, seed: Option<u64>) -> io::Result<Box<dyn IoDriver>> {
        Ok(match self {
            Clock::Real => Box::new(RealDriver::new()?),
            Clock::Virtual => Box::new(TestDriver::new(seed)),
        })
    }
}

/// 予定できるシグナルの種類。`Std.Signal.Signal` に対応する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Interrupt,
    Terminate,
}

impl Signal {
    /// 止める語と同じ `Control` のビット。
    pub(crate) fn bit(self) -> u32 {
        match self {
            Signal::Interrupt => Control::INTERRUPT,
            Signal::Terminate => Control::TERMINATE,
        }
    }
}

/// 仮想のシグナルの予定の1項。`at_ms` は driver を作ったときからのミリ秒で、driver の時計で数える。テストの
/// ハーネスが `RunConfig::with_signals` と `Machine::with_signals` で渡す (docs/spec/vm.md の「時間の待ち」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualSignal {
    pub at_ms: u64,
    pub signal: Signal,
}

/// 満期の判定で読む今の時刻。切り捨てるので、期限より早くは満ちない (docs/spec/vm.md の「時計と driver」)。
pub(crate) fn floor_ms(d: Duration) -> u64 {
    d.as_millis() as u64
}

/// 登録で読む今の時刻。切り上げるので、本物の時計で期限が `ms` より早く来ない (docs/spec/vm.md の「時計と driver」)。
pub(crate) fn ceil_ms(d: Duration) -> u64 {
    d.as_millis() as u64 + u64::from(!d.subsec_nanos().is_multiple_of(1_000_000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_reads_whole_milliseconds() {
        assert_eq!(ceil_ms(Duration::from_micros(900)), 1);
        assert_eq!(floor_ms(Duration::from_micros(900)), 0);
        assert_eq!(ceil_ms(Duration::from_millis(7)), 7);
    }
}
