//! worker 0の判定 (docs/implementation/architecture.mdの「`eml_steps`の内部」)。

#[allow(unused_imports)]
use vstd::prelude::*;

verus! {

/// 各効果はシェルの1回のアトミックな読み、1つの区間か1回の待ちで、シェルはそこで読んだものだけを返す。worker 0は`Park`
/// から、起こす側か自分から戻るdriverの待ちが終わらせるまで眠った形のままであり、遅くとも判定の最後の効果で終わる。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Effect {
    /// `running`を読む。
    Count,
    /// すべての欄の`listed`をlockなしで読む。`Count`がほかのworkerを読まなかった後にだけ出す。
    Lists,
    /// 登録表の1つの区間: 期限、driverとプールの仕事にある操作、シグナルを待つタスク、根のscope。
    Registry,
    /// 自分の欄の区間。終わりか時計を進める前の最後に出す: 受け箱、`woken`、もう一度`running`、mail、`NOTICE`、止める語
    /// 、`holds`と`owed`。後の2つは`running`の後に読む。
    Own,
    /// 起こされるまで条件変数で待つ。
    Wait,
    /// 本物の時計: worker 0のdriverか条件変数で、自分の期限か起こしまで待つ。過ぎた期限はループに戻る理由にならない。待ちは
    /// すぐに戻り、戻った待ちがそれを集める。
    WaitUntil,
    /// 仮想の時計: 次の期限までのdriverの待ち。
    Advance,
    Restart,
    RootClosed,
    Deadlock,
    Stop,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum RootFact {
    None,
    Open,
    Closed,
}

/// 登録表の区間が読んだもの。自分の欄の区間が決めるまで持つ。`timer`: どこかの区画に期限がある。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Registered {
    pub timer: bool,
    pub ops: bool,
    pub signal_waiters: bool,
    pub root: RootFact,
}

/// シェルが直前の効果で読んだもの。`stalled`: driverの待ちが、何も進められないと答えた。そう答えるのは期限のない待ちだ
/// けで、シェルが期限を渡さないのはrunのすべての操作がworker 0のdriverにあるときだけなので、判定はそのとき`ops`を数えな
/// くてよい。この答えでは、シェルはworker 0の眠った形をそのままにする。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Seen {
    pub others_running: bool,
    pub listed: bool,
    pub timer: bool,
    pub ops: bool,
    pub signal_waiters: bool,
    pub root: RootFact,
    pub inbox: bool,
    pub woken: bool,
    pub mail: bool,
    pub notice: bool,
    pub stop: bool,
    pub holds: bool,
    /// `Own`: `running`を0にした`Park`が、まだ`WakeZero`をしていない。その間`running`は0でworker 0はまだ起こされておら
    /// ず、読んだ事実は眠ったworkerの最後の項目より古いことがある。
    pub owed: bool,
    pub stalled: bool,
}

/// 各値はシェルが直前に行った効果を表す。`Start`は入口で、`End`は終わりの後に来る。シェルはその先へ進めない。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum At {
    Start,
    Counted,
    Listed,
    Busy,
    Registered,
    Owned(Registered),
    Waited,
    WaitedUntil,
    Advanced,
    End,
}

/// `virtual_clock`はrunの間変わらない。`stalled`はdriverの待ちがそう答えたら立ったままになる。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Pc {
    pub at: At,
    pub virtual_clock: bool,
    pub stalled: bool,
}

pub open spec fn to(pc: Pc, at: At, e: Effect) -> (Pc, Effect) {
    (Pc { at, virtual_clock: pc.virtual_clock, stalled: pc.stalled }, e)
}

/// どの事実も静かなときの決め方: まず閉じた根、次に時計の源。
pub open spec fn decide_spec(
    virtual_clock: bool,
    stalled: bool,
    r: Registered,
    holds: bool,
) -> Effect {
    let ops = r.ops && !stalled;
    let by_wake = r.signal_waiters || holds;
    if r.root == RootFact::Closed {
        Effect::RootClosed
    } else if virtual_clock {
        if r.timer || ops {
            Effect::Advance
        } else if by_wake {
            Effect::Wait
        } else {
            Effect::Deadlock
        }
    } else if stalled {
        if r.timer || by_wake {
            Effect::Wait
        } else {
            Effect::Deadlock
        }
    } else if r.timer || ops || by_wake {
        Effect::WaitUntil
    } else {
        Effect::Deadlock
    }
}

pub open spec fn quiet_spec(seen: Seen) -> bool {
    !(seen.inbox || seen.woken || seen.others_running || seen.mail || seen.notice || seen.owed)
}

pub open spec fn after_wait_spec(pc: Pc, seen: Seen) -> (Pc, Effect) {
    if seen.stalled {
        (Pc { at: At::Counted, virtual_clock: pc.virtual_clock, stalled: true }, Effect::Count)
    } else {
        to(pc, At::End, Effect::Restart)
    }
}

pub open spec fn step_spec(pc: Pc, seen: Seen) -> (Pc, Effect) {
    match pc.at {
        At::Start => to(pc, At::Counted, Effect::Count),
        At::Counted => if !seen.others_running {
            to(pc, At::Listed, Effect::Lists)
        } else if pc.virtual_clock || pc.stalled {
            to(pc, At::Waited, Effect::Wait)
        } else {
            to(pc, At::Busy, Effect::Registry)
        },
        At::Listed => if seen.listed {
            to(pc, At::End, Effect::Restart)
        } else {
            to(pc, At::Registered, Effect::Registry)
        },
        At::Busy => to(pc, At::WaitedUntil, Effect::WaitUntil),
        At::Registered => to(
            pc,
            At::Owned(
                Registered {
                    timer: seen.timer,
                    ops: seen.ops,
                    signal_waiters: seen.signal_waiters,
                    root: seen.root,
                },
            ),
            Effect::Own,
        ),
        At::Owned(r) => if seen.stop {
            to(pc, At::End, Effect::Stop)
        } else if !quiet_spec(seen) {
            to(pc, At::End, Effect::Restart)
        } else {
            let e = decide_spec(pc.virtual_clock, pc.stalled, r, seen.holds);
            if e == Effect::Advance {
                to(pc, At::Advanced, e)
            } else if e == Effect::WaitUntil {
                to(pc, At::WaitedUntil, e)
            } else if e == Effect::Wait {
                to(pc, At::Waited, e)
            } else {
                to(pc, At::End, e)
            }
        },
        At::Waited => to(pc, At::End, Effect::Restart),
        At::WaitedUntil => after_wait_spec(pc, seen),
        At::Advanced => after_wait_spec(pc, seen),
        At::End => to(pc, At::End, Effect::Restart),
    }
}

fn decide(virtual_clock: bool, stalled: bool, r: Registered, holds: bool) -> (e: Effect)
    ensures
        e == decide_spec(virtual_clock, stalled, r, holds),
{
    let ops = r.ops && !stalled;
    let by_wake = r.signal_waiters || holds;
    if matches!(r.root, RootFact::Closed) {
        return Effect::RootClosed;
    }
    if virtual_clock {
        if r.timer || ops {
            Effect::Advance
        } else if by_wake {
            Effect::Wait
        } else {
            Effect::Deadlock
        }
    } else if stalled {
        if r.timer || by_wake {
            Effect::Wait
        } else {
            Effect::Deadlock
        }
    } else if r.timer || ops || by_wake {
        Effect::WaitUntil
    } else {
        Effect::Deadlock
    }
}

fn go(pc: Pc, at: At, e: Effect) -> (r: (Pc, Effect))
    ensures
        r == to(pc, at, e),
{
    (Pc { at, virtual_clock: pc.virtual_clock, stalled: pc.stalled }, e)
}

fn after_wait(pc: Pc, seen: Seen) -> (r: (Pc, Effect))
    ensures
        r == after_wait_spec(pc, seen),
{
    if seen.stalled {
        (Pc { at: At::Counted, virtual_clock: pc.virtual_clock, stalled: true }, Effect::Count)
    } else {
        go(pc, At::End, Effect::Restart)
    }
}

pub fn step(pc: Pc, seen: Seen) -> (r: (Pc, Effect))
    ensures
        r == step_spec(pc, seen),
{
    match pc.at {
        At::Start => go(pc, At::Counted, Effect::Count),
        At::Counted => if !seen.others_running {
            go(pc, At::Listed, Effect::Lists)
        } else if pc.virtual_clock || pc.stalled {
            go(pc, At::Waited, Effect::Wait)
        } else {
            go(pc, At::Busy, Effect::Registry)
        },
        At::Listed => if seen.listed {
            go(pc, At::End, Effect::Restart)
        } else {
            go(pc, At::Registered, Effect::Registry)
        },
        At::Busy => go(pc, At::WaitedUntil, Effect::WaitUntil),
        At::Registered => go(
            pc,
            At::Owned(
                Registered {
                    timer: seen.timer,
                    ops: seen.ops,
                    signal_waiters: seen.signal_waiters,
                    root: seen.root,
                },
            ),
            Effect::Own,
        ),
        At::Owned(r) => if seen.stop {
            go(pc, At::End, Effect::Stop)
        } else if seen.inbox || seen.woken || seen.others_running || seen.mail || seen.notice
            || seen.owed {
            go(pc, At::End, Effect::Restart)
        } else {
            let e = decide(pc.virtual_clock, pc.stalled, r, seen.holds);
            match e {
                Effect::Advance => go(pc, At::Advanced, e),
                Effect::WaitUntil => go(pc, At::WaitedUntil, e),
                Effect::Wait => go(pc, At::Waited, e),
                _ => go(pc, At::End, e),
            }
        },
        At::Waited => go(pc, At::End, Effect::Restart),
        At::WaitedUntil => after_wait(pc, seen),
        At::Advanced => after_wait(pc, seen),
        At::End => go(pc, At::End, Effect::Restart),
    }
}

/// 自分の欄が静かで止めるビットがなければ、期限、操作、シグナルを待つタスク、`holds`が何であっても、閉じた根が判定を終
/// える。デッドロックにはすべての源が静かである必要がある。
pub proof fn step_keeps_the_rules(pc: Pc, seen: Seen)
    ensures
        pc.at is Owned && pc.at->Owned_0.root == RootFact::Closed && !seen.stop && quiet_spec(seen)
            ==> step_spec(pc, seen).1 == Effect::RootClosed,
        pc.at is Owned && step_spec(pc, seen).1 == Effect::Deadlock ==> {
            let r = pc.at->Owned_0;
            &&& r.root != RootFact::Closed
            &&& !r.timer
            &&& !r.ops || pc.stalled
            &&& !r.signal_waiters
            &&& !seen.holds
            &&& quiet_spec(seen)
            &&& !seen.stop
        },
        step_spec(pc, seen).1 == Effect::Deadlock || step_spec(pc, seen).1 == Effect::RootClosed
            || step_spec(pc, seen).1 == Effect::Advance ==> pc.at is Owned,
        !pc.virtual_clock ==> step_spec(pc, seen).1 != Effect::Advance,
        !pc.virtual_clock && !pc.stalled ==> step_spec(pc, seen).1 != Effect::Wait,
        pc.stalled && !(pc.at is Busy) ==> step_spec(pc, seen).1 != Effect::WaitUntil,
        step_spec(pc, seen).0.at is Busy ==> !step_spec(pc, seen).0.stalled,
        pc.stalled && step_spec(pc, seen).1 == Effect::Advance ==> pc.at is Owned
            && pc.at->Owned_0.timer,
        step_spec(pc, seen).0.stalled == (pc.stalled || ((pc.at is WaitedUntil || pc.at is Advanced)
            && seen.stalled)),
{
}

} // verus!

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{At, Effect, Pc, RootFact, Seen, step};

    fn quiet() -> Seen {
        Seen {
            others_running: false,
            listed: false,
            timer: false,
            ops: false,
            signal_waiters: false,
            root: RootFact::Open,
            inbox: false,
            woken: false,
            mail: false,
            notice: false,
            stop: false,
            holds: false,
            owed: false,
            stalled: false,
        }
    }

    /// 判定を`Start`から流し、効果ごとに`feed`が返す読みを返して、効果の列を集める。終わりの効果で止まる。
    fn run(virtual_clock: bool, feed: impl Fn(Effect) -> Seen) -> Vec<Effect> {
        let mut pc = Pc {
            at: At::Start,
            virtual_clock,
            stalled: false,
        };
        let mut seen = quiet();
        let mut effects = Vec::new();
        loop {
            let (next, effect) = step(pc, seen);
            effects.push(effect);
            match effect {
                Effect::Restart
                | Effect::RootClosed
                | Effect::Deadlock
                | Effect::Stop
                | Effect::Wait
                | Effect::WaitUntil
                | Effect::Advance => return effects,
                _ => {}
            }
            assert!(effects.len() < 16, "{effects:?}");
            pc = next;
            seen = feed(effect);
        }
    }

    /// `run`と同じだが、driverの待ち(`WaitUntil`、`Advance`)では終わらない。その答えも`feed`から返す。
    fn through(virtual_clock: bool, feed: impl Fn(Effect) -> Seen) -> Vec<Effect> {
        let mut pc = Pc {
            at: At::Start,
            virtual_clock,
            stalled: false,
        };
        let mut seen = quiet();
        let mut effects = Vec::new();
        loop {
            let (next, effect) = step(pc, seen);
            effects.push(effect);
            match effect {
                Effect::Restart
                | Effect::RootClosed
                | Effect::Deadlock
                | Effect::Stop
                | Effect::Wait => return effects,
                _ => {}
            }
            assert!(effects.len() < 16, "{effects:?}");
            pc = next;
            seen = feed(effect);
        }
    }

    fn reads(sources: Seen) -> impl Fn(Effect) -> Seen {
        move |_| sources
    }

    #[test]
    fn a_deadlock_needs_every_source_quiet() {
        use Effect::*;
        for virtual_clock in [false, true] {
            assert_eq!(
                run(virtual_clock, reads(quiet())),
                [Count, Lists, Registry, Own, Deadlock]
            );
        }
        let raised = [
            (
                Seen {
                    timer: true,
                    ..quiet()
                },
                Advance,
            ),
            (
                Seen {
                    ops: true,
                    ..quiet()
                },
                Advance,
            ),
            (
                Seen {
                    signal_waiters: true,
                    ..quiet()
                },
                Wait,
            ),
            (
                Seen {
                    holds: true,
                    ..quiet()
                },
                Wait,
            ),
        ];
        for (sources, on_virtual) in raised {
            assert_eq!(run(true, reads(sources)).last(), Some(&on_virtual));
            assert_eq!(run(false, reads(sources)).last(), Some(&WaitUntil));
        }
        // 自分の欄の読みのどれかが静かでなければ、判定は集め直す
        let own = [
            Seen {
                inbox: true,
                ..quiet()
            },
            Seen {
                woken: true,
                ..quiet()
            },
            Seen {
                mail: true,
                ..quiet()
            },
            Seen {
                notice: true,
                ..quiet()
            },
            Seen {
                owed: true,
                ..quiet()
            },
        ];
        for facts in own {
            assert_eq!(
                run(true, reads(facts)),
                [Count, Lists, Registry, Own, Restart]
            );
        }
        assert_eq!(
            run(
                true,
                reads(Seen {
                    listed: true,
                    ..quiet()
                })
            ),
            [Count, Lists, Restart]
        );
        // driverが進めないと答えた操作は数えない。答えはdeadlineのない待ちからしか来ない
        let stalled = |effect| match effect {
            Advance => Seen {
                ops: true,
                stalled: true,
                ..quiet()
            },
            _ => Seen {
                ops: true,
                ..quiet()
            },
        };
        assert_eq!(
            through(true, stalled),
            [
                Count, Lists, Registry, Own, Advance, Count, Lists, Registry, Own, Deadlock
            ]
        );
    }

    #[test]
    fn a_closed_root_wins_over_timers_operations_waiters_and_holds() {
        use Effect::*;
        let everything = Seen {
            root: RootFact::Closed,
            timer: true,
            ops: true,
            signal_waiters: true,
            holds: true,
            ..quiet()
        };
        for virtual_clock in [false, true] {
            assert_eq!(
                run(virtual_clock, reads(everything)),
                [Count, Lists, Registry, Own, RootClosed]
            );
            assert_eq!(
                run(
                    virtual_clock,
                    reads(Seen {
                        inbox: true,
                        ..everything
                    })
                )
                .last(),
                Some(&Restart)
            );
            assert_eq!(
                run(
                    virtual_clock,
                    reads(Seen {
                        stop: true,
                        inbox: true,
                        ..everything
                    })
                )
                .last(),
                Some(&Stop)
            );
        }
    }

    #[test]
    fn the_real_clock_waits_on_the_driver_while_others_run() {
        use Effect::*;
        let busy = Seen {
            others_running: true,
            ..quiet()
        };
        assert_eq!(run(false, reads(busy)), [Count, Registry, WaitUntil]);
        assert_eq!(
            run(
                false,
                reads(Seen {
                    timer: true,
                    ..busy
                })
            ),
            [Count, Registry, WaitUntil]
        );
        assert_eq!(run(true, reads(busy)), [Count, Wait]);
    }

    #[test]
    fn a_stalled_driver_is_not_waited_on_again() {
        use Effect::*;
        let after_stall = |sources: Seen, others: bool| {
            let stalled = Cell::new(false);
            move |effect| match effect {
                WaitUntil => {
                    stalled.set(true);
                    Seen {
                        stalled: true,
                        ..sources
                    }
                }
                Count => Seen {
                    others_running: others && stalled.get(),
                    ..sources
                },
                _ => sources,
            }
        };
        let ops = Seen {
            ops: true,
            ..quiet()
        };
        assert_eq!(
            through(false, after_stall(ops, false)),
            [
                Count, Lists, Registry, Own, WaitUntil, Count, Lists, Registry, Own, Deadlock
            ]
        );
        let waiter = Seen {
            signal_waiters: true,
            ..ops
        };
        assert_eq!(
            through(false, after_stall(waiter, false)).last(),
            Some(&Wait)
        );
        assert_eq!(
            through(false, after_stall(ops, true)),
            [Count, Lists, Registry, Own, WaitUntil, Count, Wait]
        );
    }
}
