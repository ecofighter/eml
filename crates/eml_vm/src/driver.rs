//! 本物の時計と仮想の時計、その選び方 `Clock` (docs/spec/vm.md の「時計と driver」)。

use std::str::FromStr;
use std::time::{Duration, Instant};

use eml_rt::IoDriver;

/// 本物の時計が1回の `wait_until` で眠る長さの上限。空きのループは起きるたびに止める語を読むので、Ctrl-C と確保量の
/// 上限はどの方針でもこの長さのうちに眠る worker を止める (docs/spec/vm.md の「時計と driver」)。
pub(crate) const IDLE_TICK: Duration = Duration::from_millis(10);

/// 単調な時計 `Instant` で待つ driver。
pub(crate) struct RealClock {
    epoch: Instant,
}

impl RealClock {
    pub(crate) fn new() -> RealClock {
        RealClock {
            epoch: Instant::now(),
        }
    }
}

impl IoDriver for RealClock {
    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }

    fn wait_until(&mut self, deadline_ms: u64) {
        let now = self.now();
        let target = Duration::from_millis(deadline_ms);
        if now < target {
            std::thread::sleep((target - now).min(IDLE_TICK));
        }
    }
}

/// すべてのタスクが待つときにだけ、いちばん早い期限まで進む時計。0 から始まり、ミリ秒の整数で進む。
#[derive(Default)]
pub(crate) struct VirtualClock {
    now_ms: u64,
}

impl IoDriver for VirtualClock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.now_ms)
    }

    // 時計は戻らないので、過ぎた期限では進めない
    fn wait_until(&mut self, deadline_ms: u64) {
        self.now_ms = self.now_ms.max(deadline_ms);
    }
}

/// タイマーが使う時計の選び方。`RunConfig::with_clock` と `Machine::with_clock` で選ぶ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Clock {
    /// 単調な本物の時計で眠る。
    Real,
    /// すべてのタスクが待つときにだけ進む。
    #[default]
    Virtual,
}

impl Clock {
    /// この時計の新しい driver。
    pub(crate) fn driver(self) -> Box<dyn IoDriver> {
        match self {
            Clock::Real => Box::new(RealClock::new()),
            Clock::Virtual => Box::new(VirtualClock::default()),
        }
    }
}

impl FromStr for Clock {
    type Err = String;

    /// `real` か `virtual` を読む。
    fn from_str(text: &str) -> Result<Clock, String> {
        match text {
            "real" => Ok(Clock::Real),
            "virtual" => Ok(Clock::Virtual),
            _ => Err(format!("expected `real` or `virtual`, found `{text}`")),
        }
    }
}

/// 満期の判定で読む今の時刻。切り捨てるので、期限より早くは満ちない (docs/spec/vm.md の「時計と driver」)。
pub(crate) fn floor_ms(d: Duration) -> u64 {
    d.as_millis() as u64
}

/// 登録で読む今の時刻。切り上げるので、本物の時計で期限が `ms` より早く来ない (docs/spec/vm.md の「時計と driver」)。
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "時間の待ちを登録する操作だけが使う")
)]
pub(crate) fn ceil_ms(d: Duration) -> u64 {
    d.as_millis() as u64 + u64::from(!d.subsec_nanos().is_multiple_of(1_000_000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_virtual_clock_advances_only_forward() {
        let mut clock = VirtualClock::default();
        assert_eq!(clock.now(), Duration::ZERO);
        clock.wait_until(5);
        assert_eq!(clock.now(), Duration::from_millis(5));
        clock.wait_until(3);
        assert_eq!(clock.now(), Duration::from_millis(5));
    }

    #[test]
    fn a_real_clock_does_not_go_back() {
        let clock = RealClock::new();
        let first = clock.now();
        assert!(clock.now() >= first);
    }

    /// 早く戻ってよい `wait_until` を、呼ぶ側が確かめ直して繰り返せば、期限に着く。
    #[test]
    fn a_real_clock_reaches_a_deadline() {
        let mut clock = RealClock::new();
        let deadline = ceil_ms(clock.now()) + 5;
        loop {
            clock.wait_until(deadline);
            if floor_ms(clock.now()) >= deadline {
                break;
            }
        }
    }

    #[test]
    fn rounding_reads_whole_milliseconds() {
        assert_eq!(ceil_ms(Duration::from_micros(900)), 1);
        assert_eq!(floor_ms(Duration::from_micros(900)), 0);
        assert_eq!(ceil_ms(Duration::from_millis(7)), 7);
    }
}
