//! 空きの手順 (docs/implementation/architecture.mdの「`eml_steps`の内部」)。

#[allow(unused_imports)]
use vstd::prelude::*;

verus! {

/// シェルの1回のアトミックな操作か、1つのlockの区間。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Effect {
    /// 出す側の自分の欄の区間: 仕事を積み、`listed`を上げる。
    Push,
    /// 出す側が`sleepers > 0 && searching == 0`を読む。
    Probe,
    /// 眠りの集合の区間: 載ったworkerを1つ取り出す。
    PassOn,
    /// 取り出したworkerの欄の区間: `woken`を立て、眠った形なら形を終えて`running`に数える。鳴らすのは区間の後である。
    Wake,
    IncSleepers,
    DecSleepers,
    IncSearching,
    DecSearching,
    Enlist,
    Delist,
    /// ほかの欄をそれぞれのlockの下で1回ずつ訪れ、仕事のある最初の一覧から最も古い仕事を取る。
    Scan,
    /// 自分の欄の区間: 受け箱、`woken`、止める語を読み直す。どれも静かなら眠った形になって`running`から抜け、
    /// それで`running`が0になったかを返す。worker 0以外のworkerは、`running`を減らす前にrunの`owed`を上げ、減らしても0に
    /// ならなかったときは同じ区間で下げ直す。`Own`は別のlockの下で`running`、次に`owed`を読むので、減らした後に上げると、
    /// `running`が0、`owed`が0で、worker 0がまだ起こされていない瞬間が残る。
    Park,
    /// `Wake`と同じようにworker 0を起こし、worker 0の欄の同じ区間で`owed`を下げる。
    WakeZero,
    /// 眠った形が終わるまで待つ。worker 0ではこれが判定である。
    Sleep,
    /// 取った仕事があればそれを持って、ループに戻る。
    Work,
}

/// 手順のどこにいるか。各値はシェルが直前に行った効果を表し、始めの値`Publish`、`Idle`、`MainPark`は入口を表す。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum At {
    Publish,
    Pushed,
    Probed,
    PassedOn,
    Woke,
    Idle,
    /// `rescan`: 規則3の探索が何も取らず、workerは走査の前に自分を載せ直す。
    RaisedSleepers { rescan: bool },
    Enlisted { rescan: bool },
    LoweredSearching,
    Scanned,
    Parked,
    WokeZero,
    Slept,
    /// `search`: workerはほかの仕事なしに眠りを抜け、また眠る前に探索する。
    Delisted { search: bool },
    LoweredSleepers { search: bool },
    RaisedSearching,
    Searched,
    Found,
    MainPark,
    MainParked,
    MainWokeZero,
    MainSlept,
    Done,
}

/// `worker_0`: workerは判定を眠りとして流すので、worker 0を自分で起こすことはない。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Pc {
    pub at: At,
    pub worker_0: bool,
}

/// シェルが直前の効果で読んだもの: `Probe`で`probe`、`PassOn`で`popped`、`Scan`で`took`、`Park`で`quiet` (workerが眠っ
/// た形になった) と`zero` (`running`から抜けて0にした)。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Seen {
    pub probe: bool,
    pub popped: bool,
    pub took: bool,
    pub quiet: bool,
    pub zero: bool,
    /// `other`は、何かを見つけた`Park`と`Sleep`が読む: 受け箱に知らせがあるか、止めるビットが立っているか、待ちが自分で
    /// 戻った。
    pub other: bool,
}

pub open spec fn to(pc: Pc, at: At, e: Effect) -> (Pc, Effect) {
    (Pc { at, worker_0: pc.worker_0 }, e)
}

pub open spec fn step_spec(pc: Pc, seen: Seen) -> (Pc, Effect) {
    match pc.at {
        At::Publish => to(pc, At::Pushed, Effect::Push),
        At::Pushed => to(pc, At::Probed, Effect::Probe),
        At::Probed => if seen.probe {
            to(pc, At::PassedOn, Effect::PassOn)
        } else {
            to(pc, At::Done, Effect::Work)
        },
        At::PassedOn => if seen.popped {
            to(pc, At::Woke, Effect::Wake)
        } else {
            to(pc, At::Done, Effect::Work)
        },
        At::Woke => to(pc, At::Done, Effect::Work),
        At::Idle => to(pc, At::RaisedSleepers { rescan: false }, Effect::IncSleepers),
        At::RaisedSleepers { rescan } => to(pc, At::Enlisted { rescan }, Effect::Enlist),
        At::Enlisted { rescan } => if rescan {
            to(pc, At::LoweredSearching, Effect::DecSearching)
        } else {
            to(pc, At::Scanned, Effect::Scan)
        },
        At::LoweredSearching => to(pc, At::Scanned, Effect::Scan),
        At::Scanned => if seen.took {
            to(pc, At::Delisted { search: false }, Effect::Delist)
        } else {
            to(pc, At::Parked, Effect::Park)
        },
        At::Parked => if !seen.quiet {
            to(pc, At::Delisted { search: !seen.other }, Effect::Delist)
        } else if seen.zero && !pc.worker_0 {
            to(pc, At::WokeZero, Effect::WakeZero)
        } else {
            to(pc, At::Slept, Effect::Sleep)
        },
        At::WokeZero => to(pc, At::Slept, Effect::Sleep),
        At::Slept => to(pc, At::Delisted { search: !(pc.worker_0 || seen.other) }, Effect::Delist),
        At::Delisted { search } => to(pc, At::LoweredSleepers { search }, Effect::DecSleepers),
        At::LoweredSleepers { search } => if search {
            to(pc, At::RaisedSearching, Effect::IncSearching)
        } else {
            to(pc, At::PassedOn, Effect::PassOn)
        },
        At::RaisedSearching => to(pc, At::Searched, Effect::Scan),
        At::Searched => if seen.took {
            to(pc, At::Found, Effect::DecSearching)
        } else {
            to(pc, At::RaisedSleepers { rescan: true }, Effect::IncSleepers)
        },
        At::Found => to(pc, At::PassedOn, Effect::PassOn),
        At::MainPark => to(pc, At::MainParked, Effect::Park),
        At::MainParked => if !seen.quiet {
            to(pc, At::Done, Effect::Work)
        } else if seen.zero {
            to(pc, At::MainWokeZero, Effect::WakeZero)
        } else {
            to(pc, At::MainSlept, Effect::Sleep)
        },
        At::MainWokeZero => to(pc, At::MainSlept, Effect::Sleep),
        At::MainSlept => to(pc, At::Done, Effect::Work),
        At::Done => to(pc, At::Done, Effect::Work),
    }
}

pub fn step(pc: Pc, seen: Seen) -> (r: (Pc, Effect))
    ensures
        r == step_spec(pc, seen),
{
    let (at, e) = match pc.at {
        At::Publish => (At::Pushed, Effect::Push),
        At::Pushed => (At::Probed, Effect::Probe),
        At::Probed => if seen.probe {
            (At::PassedOn, Effect::PassOn)
        } else {
            (At::Done, Effect::Work)
        },
        At::PassedOn => if seen.popped {
            (At::Woke, Effect::Wake)
        } else {
            (At::Done, Effect::Work)
        },
        At::Woke => (At::Done, Effect::Work),
        At::Idle => (At::RaisedSleepers { rescan: false }, Effect::IncSleepers),
        At::RaisedSleepers { rescan } => (At::Enlisted { rescan }, Effect::Enlist),
        At::Enlisted { rescan } => if rescan {
            (At::LoweredSearching, Effect::DecSearching)
        } else {
            (At::Scanned, Effect::Scan)
        },
        At::LoweredSearching => (At::Scanned, Effect::Scan),
        At::Scanned => if seen.took {
            (At::Delisted { search: false }, Effect::Delist)
        } else {
            (At::Parked, Effect::Park)
        },
        At::Parked => if !seen.quiet {
            (At::Delisted { search: !seen.other }, Effect::Delist)
        } else if seen.zero && !pc.worker_0 {
            (At::WokeZero, Effect::WakeZero)
        } else {
            (At::Slept, Effect::Sleep)
        },
        At::WokeZero => (At::Slept, Effect::Sleep),
        At::Slept => (At::Delisted { search: !(pc.worker_0 || seen.other) }, Effect::Delist),
        At::Delisted { search } => (At::LoweredSleepers { search }, Effect::DecSleepers),
        At::LoweredSleepers { search } => if search {
            (At::RaisedSearching, Effect::IncSearching)
        } else {
            (At::PassedOn, Effect::PassOn)
        },
        At::RaisedSearching => (At::Searched, Effect::Scan),
        At::Searched => if seen.took {
            (At::Found, Effect::DecSearching)
        } else {
            (At::RaisedSleepers { rescan: true }, Effect::IncSleepers)
        },
        At::Found => (At::PassedOn, Effect::PassOn),
        At::MainPark => (At::MainParked, Effect::Park),
        At::MainParked => if !seen.quiet {
            (At::Done, Effect::Work)
        } else if seen.zero {
            (At::MainWokeZero, Effect::WakeZero)
        } else {
            (At::MainSlept, Effect::Sleep)
        },
        At::MainWokeZero => (At::MainSlept, Effect::Sleep),
        At::MainSlept => (At::Done, Effect::Work),
        At::Done => (At::Done, Effect::Work),
    };
    (Pc { at, worker_0: pc.worker_0 }, e)
}

} // verus!
