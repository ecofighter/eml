//! 安全点で、深さが今の上限を超えたときの判定 (docs/spec/vm.mdの「タスクの上限」)。

#[allow(unused_imports)]
use vstd::prelude::*;

verus! {

/// 動いているコードの上限の状態 (docs/spec/vm.mdの「タスクの上限」の表)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Limit {
    Stops,
    Task,
    Aborting,
}

/// `Defers`はEPの補助の関数のエントリである。その`evv`はプログラムのどの地点のものでもなく、そこで起こすとプログラムに見えない
/// handlerに届くので、起こすのは次のエントリまで延ばす。ネイティブの安全点はどれも関数のエントリなので、`Entry`はほかのすべての
/// 安全点を表す。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Site {
    Entry,
    Defers,
}

/// `beyond_grace`: 深さが`real + LIMIT_HEADROOM`を超えている。`over_real`: 深さが`real`を超えている。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    pub class: Limit,
    pub yielding: bool,
    pub site: Site,
    pub beyond_grace: bool,
    pub over_real: bool,
}

/// `Stop`は`DepthLimit`でrunを止め、`Raise`はタスクに`ResourceLimit`を起こす。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    Stay,
    Stop,
    Raise,
}

/// ネイティブの安全点に当てはめたdocs/spec/vm.mdの「タスクの上限」の手順。ネイティブの安全点はどれも関数のエントリなので、
/// エントリでない安全点の手順はない。本物のスタックからは食い違わない`Probe`だけが来るが、食い違うもの (`over_real`でない
/// `beyond_grace`) にも結果を返すので、呼び手が満たす前提はない。
pub open spec fn hit_spec(p: Probe) -> Hit {
    match p.class {
        Limit::Stops => if p.over_real {
            Hit::Stop
        } else {
            Hit::Stay
        },
        Limit::Aborting => if p.beyond_grace {
            Hit::Stop
        } else {
            Hit::Stay
        },
        Limit::Task => if p.yielding {
            Hit::Stay
        } else if p.beyond_grace {
            Hit::Stop
        } else if p.site == Site::Defers {
            Hit::Stay
        } else if p.over_real {
            Hit::Raise
        } else {
            Hit::Stay
        },
    }
}

pub fn hit(p: Probe) -> (r: Hit)
    ensures
        r == hit_spec(p),
{
    match p.class {
        Limit::Stops => if p.over_real {
            Hit::Stop
        } else {
            Hit::Stay
        },
        Limit::Aborting => if p.beyond_grace {
            Hit::Stop
        } else {
            Hit::Stay
        },
        Limit::Task => if p.yielding {
            Hit::Stay
        } else if p.beyond_grace {
            Hit::Stop
        } else {
            match p.site {
                Site::Defers => Hit::Stay,
                Site::Entry => if p.over_real {
                    Hit::Raise
                } else {
                    Hit::Stay
                },
            }
        },
    }
}

/// 起こすには`Ctx`が1つだけ持つyieldの状態が要り、`Aborting`の間はすでに巻き戻しているので、どちらでも起こさない。
pub proof fn hit_keeps_the_rules(p: Probe)
    ensures
        p.class == Limit::Stops && p.over_real ==> hit_spec(p) == Hit::Stop,
        p.yielding || p.class == Limit::Aborting ==> hit_spec(p) != Hit::Raise,
        p.site == Site::Defers ==> hit_spec(p) != Hit::Raise,
{
}

} // verus!

#[cfg(test)]
mod tests {
    use super::{Hit, Limit, Probe, Site, hit};

    // docs/spec/vm.mdの「タスクの上限」の表の状態ごとに1行を確かめ、VMの写しとこちらがずれたら分かるようにする
    #[test]
    fn the_three_rows_of_the_spec_table() {
        let probe = |class, beyond_grace, over_real| Probe {
            class,
            yielding: false,
            site: Site::Entry,
            beyond_grace,
            over_real,
        };
        assert_eq!(hit(probe(Limit::Stops, false, true)), Hit::Stop);
        assert_eq!(hit(probe(Limit::Task, false, true)), Hit::Raise);
        assert_eq!(hit(probe(Limit::Aborting, true, true)), Hit::Stop);
    }
}
