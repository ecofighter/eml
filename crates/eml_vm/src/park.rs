//! 分かれたrunのメインのworkerの待ち (docs/spec/vm.mdの「メインのworkerの待ち」)。`Park`が替えるのはメインの
//! workerの眠る道だけである。時計、デッドロックの判定、`END`はworker 0に残るので、すぐに戻らない`Park`も、wakerが
//! 鳴ったときに`run`を呼ぶか戻るなら、遅らせるのは固定したタスクとrunの最後の終わりだけである。どちらもしなければ、
//! メインのworkerが`running`に数えられたままになり、仮想の時計とデッドロックの判定も止まる。

use std::sync::Arc;
use std::time::Duration;

use eml_rt::IoWaker;

/// `Pending::run`が`Park`に返す答え。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// メインのworkerは走れるものをすべて動かし、また眠った。`Park`はwakerが鳴るか、この長さが過ぎるまで待つ
    /// (`None`では鳴るまで)。
    Wait(Option<Duration>),
    /// メインのworkerは抜けるところである。`Park`は戻らなければならない。
    Stopping,
    /// 眠った待ちの中から呼ばれなかったので、何も動かさなかった。
    Busy,
}

/// メインのworkerのdriverが待つOSの源。自分のループで待つ`Park`はこれをそのループの源に加え、読めるようになったら
/// `Pending::run`を呼ぶ (docs/spec/vm.mdの「メインのworkerの待ち」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollSource {
    /// epollかkqueueのfd。読めるようになったら、driverに取る完了か起こしがある。
    #[cfg(unix)]
    Fd(std::os::fd::RawFd),
}

/// メインのworkerを待つ仕事。`Park`は`Park::wait`でこれを受け取る。
pub trait Pending {
    /// 走れるものがなくなるまでメインのworkerのタスクを動かし、メインのworkerをまた眠らせる。動かすのは眠った待ちの
    /// 中から呼ばれたときだけで、ほかでは`Busy`を返す。固定したタスクが動くのはここか、`wait`が戻った後にメインの
    /// workerが`Park`の代わりに行う同じ`run`の中だけである。
    fn run(&mut self) -> Progress;
    /// 既定の待ち。メインのworkerがdriverを持てばそのdriverで、持たなければ欄の条件変数で待つ。1回の`Park::wait`
    /// で呼ぶのは多くても1回で、呼んだ後は`wait`から戻る。
    fn default_wait(&mut self) -> std::io::Result<()>;
    /// `Park`がrunの外の待ちを抱えているかを書く。真のままメインのworkerが眠っている間、worker 0はデッドロックと
    /// 決めない。待ちの中ならいつでも、`run`の前でも後でも呼べ、すぐに効く。ここで書いた真は、メインのworkerの次の
    /// 眠り (続く`run`の終わりか、次の待ち) の後のどの判定も読む。その眠りより前の判定は見逃しうる。偽になったら、
    /// worker 0を起こして判定し直させる。
    ///
    /// そのため、`Park`はタスクを動かさせる前、つまり`run`を呼ぶ前か`wait`から戻る前に`set_holds(true)`を立て、
    /// 何も始まらなければ後で下ろす。下ろせばworker 0を起こすので、先に立てても害はない。
    fn set_holds(&mut self, holds: bool);
    /// メインのworkerのdriverが待つOSの源。仮想の時計、Windows、driverを作る前は`None`である。fdはdriverのもので
    /// ある。`Park`がそれを見張るのは1回の`wait`の間だけで、`wait`から戻る前に見張りをやめ、閉じることはない。
    fn io(&self) -> Option<PollSource>;
}

/// メインのworkerの眠る道。`Machine`を持つスレッドで作り、そのスレッドから動かない。
pub trait Park {
    /// wakerが鳴るか、`timeout`が過ぎるか、`run`が`Stopping`を返すまで止まる。`pending.run()`を呼んでよい。待ちを
    /// 始める前に鳴っていれば、その待ちは戻るか`run`を呼ぶ。`run`が`Stopping`を返した後は戻る。それより早く、何も
    /// 変わらずに戻る`Park`はすぐにまた呼ばれるので、メインのスレッドを回し続ける。メインのworkerの期限を過ぎて
    /// 戻れば、`run`を呼んでいなくてもメインのworkerは起きてタイマーを拾う。`Park`が積まれた仕事、期限の来た
    /// タイマー、止めることを残して戻れば、メインのworkerは次の`wait`の前に、`run`と同じ規則で`Park`の代わりに
    /// その仕事を動かす。そのため、戻ることもタスクを動かさせることになる (`Pending::set_holds`)。ただし、メインの
    /// workerのdriverの完了はこれに含まない。`Pending::io`と`timeout`の10 msの上限は、自分で`run`を呼ぶ`Park`の
    /// ためにある。その`Park`は源で待ち、源が読めるようになるか`timeout`が過ぎたら`run`を呼ぶ。戻るだけの`Park`の
    /// 下では、メインのworkerが自分のI/Oを拾うのは、欄が鳴ったときか自分の期限を過ぎたときだけである
    /// (docs/spec/vm.mdの「メインのworkerの待ち」)。
    fn wait(&mut self, timeout: Option<Duration>, pending: &mut dyn Pending)
    -> std::io::Result<()>;
    /// 待ちを終わらせるwaker。メインのworkerを起こすたびに鳴らす。`Pending::default_wait`だけで待つ`Park`では
    /// `None`である。
    fn waker(&self) -> Option<Arc<dyn IoWaker>>;
}

/// 分かれたrunごとに、メインのworkerの`Park`を作る。`Machine`を持つスレッドで呼ぶ。
pub type ParkFactory = Arc<dyn Fn() -> Box<dyn Park> + Send + Sync>;

/// 既定の`Park`。`Pending::default_wait`だけで待つので、メインのworkerのI/Oのdriverでも待ち、それから戻る。積まれた
/// 仕事は、その後にメインのworkerが代わりに動かす。
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPark;

impl Park for DefaultPark {
    fn wait(
        &mut self,
        _timeout: Option<Duration>,
        pending: &mut dyn Pending,
    ) -> std::io::Result<()> {
        pending.default_wait()
    }

    fn waker(&self) -> Option<Arc<dyn IoWaker>> {
        None
    }
}

/// この数の`Stopping`を返した後も呼ばれたら、`Park`が`Stopping`を無視したとみなす。debugのビルドはそこで止まる。
const STOPPING_CALLS: u32 = 64;

/// メインのworkerの`Pending::run`の状態。入れるのは眠った待ちからだけで、`run`の中からと待ちの外からの呼び出しは
/// `Busy`である。`Stopping`の後は、待ちが終わるまでどの呼び出しにも`Stopping`を返す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Guard {
    Outside,
    Parked,
    Running,
    /// `Stopping`を返した後の呼び出しの数を持つ。
    Stopped(u32),
}

impl Guard {
    /// `Pending::run`がすることを決める。`None`なら入ってよく、状態は`Running`になる。
    pub(crate) fn enter(&mut self) -> Option<Progress> {
        match self {
            Guard::Outside | Guard::Running => Some(Progress::Busy),
            Guard::Stopped(calls) => {
                *calls += 1;
                debug_assert!(
                    *calls < STOPPING_CALLS,
                    "Park::wait ignored Progress::Stopping"
                );
                Some(Progress::Stopping)
            }
            Guard::Parked => {
                *self = Guard::Running;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Guard, Progress, STOPPING_CALLS};

    /// `run`に入れるのは眠った待ちからだけである。待ちの外からも、動いている`run`の中からも`Busy`である。
    #[test]
    fn run_pending_enters_only_from_a_parked_wait() {
        let mut guard = Guard::Outside;
        assert_eq!(guard.enter(), Some(Progress::Busy));
        assert_eq!(guard, Guard::Outside);
        guard = Guard::Parked;
        assert_eq!(guard.enter(), None);
        assert_eq!(guard, Guard::Running);
        assert_eq!(guard.enter(), Some(Progress::Busy));
        assert_eq!(guard, Guard::Running);
    }

    /// `Stopping`は待ちが終わるまで続く。待ちが終われば待ちの外の`Busy`に戻り、次の眠った待ちはまた入れる。
    #[test]
    fn stopping_is_sticky_until_the_wait_ends() {
        let mut guard = Guard::Stopped(0);
        for _ in 1..STOPPING_CALLS {
            assert_eq!(guard.enter(), Some(Progress::Stopping));
        }
        guard = Guard::Outside;
        assert_eq!(guard.enter(), Some(Progress::Busy));
        guard = Guard::Parked;
        assert_eq!(guard.enter(), None);
    }
}
