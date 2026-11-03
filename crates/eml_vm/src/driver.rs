//! 時計の選び方 `Clock` と、時刻のミリ秒への丸め (docs/spec/vm.md の「時計と driver」)。driver そのものは `eml_io` にある。

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use eml_io::{RealDriver, TestDriver};
use eml_rt::{Control, IoDriver};

use crate::VmFault;
use crate::sched::NO_DRIVER;

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
    /// この時計で始める run の時間の基準。本物の時計は今から数え、仮想の時計は 0 から数える。`Machine` が driver を
    /// 作るときに1回だけ作り、その driver と同じ基準で run が時刻を読む (docs/spec/vm.md の「時計と driver」)。
    pub(crate) fn start(self) -> RunClock {
        match self {
            Clock::Real => RunClock::Real {
                epoch: Instant::now(),
            },
            Clock::Virtual => RunClock::Virtual(Arc::new(AtomicU64::new(0))),
        }
    }

    /// この時計の出どころ。
    pub(crate) fn kind(self) -> DriverKind {
        match self {
            Clock::Real => DriverKind::Real,
            Clock::Virtual => DriverKind::Virtual,
        }
    }
}

/// run の時間の基準。run の driver はどれもこの基準で作り、VM はここからだけ時刻を読む
/// (docs/spec/vm.md の「時計と driver」)。
#[derive(Debug, Clone)]
pub(crate) enum RunClock {
    /// `epoch` から数える本物の時計。
    Real { epoch: Instant },
    /// run の driver が共有する仮想の時計。ミリ秒の整数である。
    Virtual(Arc<AtomicU64>),
    /// `Machine::with_driver` で渡した driver の時計。基準を知らないので、時刻は driver に聞く。
    Injected,
}

impl RunClock {
    /// この基準の新しい driver。`seed` は `Sched::seed` で、テストの driver が決着の組を混ぜるのに使う。driver は
    /// 作ったスレッドの上でだけ使うので、run を行うスレッドで作る。渡された driver の基準 `Injected` からは作れない。
    pub(crate) fn driver(&self, seed: Option<u64>) -> io::Result<Box<dyn IoDriver>> {
        Ok(match self {
            RunClock::Real { epoch } => Box::new(RealDriver::new_at(*epoch)?),
            RunClock::Virtual(clock) => Box::new(TestDriver::new_shared(seed, Arc::clone(clock))),
            RunClock::Injected => unreachable!("an injected driver is not made from a clock"),
        })
    }

    /// 基準が示す今の時刻。`Injected` だけは `driver` に聞く。
    pub(crate) fn now(&self, driver: Option<&dyn IoDriver>) -> Result<Duration, VmFault> {
        match self {
            RunClock::Real { epoch } => Ok(epoch.elapsed()),
            RunClock::Virtual(clock) => Ok(Duration::from_millis(clock.load(Ordering::SeqCst))),
            RunClock::Injected => driver.map(|d| d.now()).ok_or(NO_DRIVER),
        }
    }
}

/// run が使う driver の出どころ。OS のシグナルを中継するか、空きの待ちに上限を掛けるかを決める
/// (docs/spec/vm.md の「時計と driver」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DriverKind {
    /// `Machine` が `Clock::Real` から作った本物の driver。
    Real,
    /// `Machine` が `Clock::Virtual` から作ったテストの driver。空きのループで時計を進める。
    Virtual,
    /// `Machine::with_driver` で渡した driver。眠る driver として扱い、OS のシグナルは中継しない。
    Injected,
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

/// 仮想のシグナルの予定の1項。`at_ms` は run の時間の基準の始まりからのミリ秒で、run の時計で数える。テストの
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
