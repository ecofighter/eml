//! workerのループの1回の手順を決める (docs/implementation/architecture.mdの「`eml_steps`の内部」)。

#[allow(unused_imports)]
use vstd::prelude::*;

verus! {

/// workerが回すループ。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Seat {
    /// 根を流す。ループは根が終わったときに戻る。
    Worker0,
    /// ループは止めるビットでだけ戻る。
    Peer,
    /// 別のメインのworker。仕事を取らず、準備のできたものがなければ`Park`に戻る。
    Main,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Root {
    Main,
    Result,
}

/// worker 0のループが知る根。判定が根のscopeを閉じたと読んだとき`Closed`になる。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum RootState {
    Main,
    None,
    Open,
    Closed,
    Done,
}

/// `pass`は今のパスがまだ流してよい準備のできた項目の数。`detached`、`foreign`、`looked`は、前の`Inbox`の後にその手順を
/// 出したことを表す。`own`は、何も取らなかった前の`Detached`の後にこのworkerがセルの仕事を積んだことを表し、worker自身
/// が持つので共有の語からは読まない。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Facts {
    pub seat: Seat,
    pub stop: bool,
    pub root: RootState,
    pub inbox: bool,
    pub pass: u64,
    pub ready_empty: bool,
    pub own: bool,
    pub detached: bool,
    pub foreign: bool,
    pub looked: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Step {
    Stop,
    Inbox,
    Root(Root),
    Ready,
    Detached,
    Foreign,
    Look,
    Idle,
    Return,
}

pub open spec fn next_spec(f: Facts) -> Step {
    let main = f.seat == Seat::Main;
    if f.stop {
        Step::Stop
    } else if f.seat == Seat::Worker0 && f.root == RootState::Main {
        Step::Root(Root::Main)
    } else if f.seat == Seat::Worker0 && f.root == RootState::Closed {
        Step::Root(Root::Result)
    } else if f.seat == Seat::Worker0 && (f.root == RootState::None || f.root == RootState::Done) {
        Step::Return
    } else if f.pass > 0 {
        Step::Ready
    } else if !f.detached && !main {
        Step::Detached
    } else if !f.foreign && f.ready_empty && !main {
        Step::Foreign
    } else if !f.looked {
        Step::Look
    } else if f.inbox || !f.ready_empty || (f.own && !main) {
        Step::Inbox
    } else if main {
        Step::Return
    } else {
        Step::Idle
    }
}

pub fn next(f: Facts) -> (r: Step)
    ensures
        r == next_spec(f),
{
    let main = matches!(f.seat, Seat::Main);
    let worker_0 = matches!(f.seat, Seat::Worker0);
    if f.stop {
        return Step::Stop;
    }
    if worker_0 {
        match f.root {
            RootState::Main => return Step::Root(Root::Main),
            RootState::Closed => return Step::Root(Root::Result),
            RootState::None => return Step::Return,
            RootState::Done => return Step::Return,
            RootState::Open => {},
        }
    }
    if f.pass > 0 {
        Step::Ready
    } else if !f.detached && !main {
        Step::Detached
    } else if !f.foreign && f.ready_empty && !main {
        Step::Foreign
    } else if !f.looked {
        Step::Look
    } else if f.inbox || !f.ready_empty || (f.own && !main) {
        Step::Inbox
    } else if main {
        Step::Return
    } else {
        Step::Idle
    }
}

/// ほかのworkerの仕事は、準備のできた列が空のときにだけ出し、メインのworkerには出さない。パスは次の`Inbox`か`Idle`の前
/// に`Detached`で終わり、始めの数の間だけ流れる。`Idle`はほかにすることがないときにだけ来る。`Return`は止められていない
/// `Peer`以外のworkerにだけ来る: 根がないか終わったworker 0と、ほかにすることのないメインのworkerである。
pub proof fn next_keeps_the_rules(f: Facts)
    ensures
        next_spec(f) == Step::Foreign ==> f.ready_empty && f.seat != Seat::Main,
        next_spec(f) == Step::Detached ==> f.seat != Seat::Main,
        next_spec(f) == Step::Ready ==> f.pass > 0,
        next_spec(f) == Step::Inbox ==> f.pass == 0 && (f.detached || f.seat == Seat::Main),
        next_spec(f) == Step::Idle ==> {
            &&& !f.stop
            &&& f.seat != Seat::Main
            &&& f.seat == Seat::Worker0 ==> f.root == RootState::Open
            &&& f.pass == 0
            &&& f.ready_empty
            &&& !f.inbox
            &&& !f.own
            &&& f.detached
            &&& f.foreign
            &&& f.looked
        },
        f.stop ==> next_spec(f) == Step::Stop,
        next_spec(f) == Step::Return ==> !f.stop && f.seat != Seat::Peer && (f.seat == Seat::Worker0
            ==> f.root == RootState::None || f.root == RootState::Done) && (f.seat == Seat::Main
            ==> f.pass == 0 && f.ready_empty && !f.inbox && f.looked),
{
}

} // verus!

#[cfg(test)]
mod tests {
    use super::{Facts, Root, RootState, Seat, Step, next};

    fn quiet(seat: Seat) -> Facts {
        Facts {
            seat,
            stop: false,
            root: RootState::Open,
            inbox: false,
            pass: 0,
            ready_empty: true,
            own: false,
            detached: true,
            foreign: true,
            looked: true,
        }
    }

    #[test]
    fn the_main_worker_is_never_offered_a_job() {
        let fresh = Facts {
            detached: false,
            foreign: false,
            looked: false,
            own: true,
            ..quiet(Seat::Main)
        };
        assert_eq!(next(fresh), Step::Look);
        assert_eq!(
            next(Facts {
                looked: true,
                ..fresh
            }),
            Step::Return
        );
        let peer = Facts {
            seat: Seat::Peer,
            ..fresh
        };
        assert_eq!(next(peer), Step::Detached);
        assert_eq!(
            next(Facts {
                detached: true,
                ..peer
            }),
            Step::Foreign
        );
        let worker_0 = Facts {
            seat: Seat::Worker0,
            detached: true,
            ..fresh
        };
        assert_eq!(next(worker_0), Step::Foreign);
    }

    #[test]
    fn a_ready_pass_is_bounded_by_its_start() {
        let passing = Facts {
            pass: 2,
            ready_empty: false,
            detached: false,
            foreign: false,
            looked: false,
            ..quiet(Seat::Peer)
        };
        assert_eq!(next(passing), Step::Ready);
        // パスの間に積まれた項目は、次の`Inbox`が始めるパスまで待つ
        let spent = Facts { pass: 0, ..passing };
        assert_eq!(next(spent), Step::Detached);
        let offered = Facts {
            detached: true,
            ..spent
        };
        assert_eq!(next(offered), Step::Look);
        assert_eq!(
            next(Facts {
                looked: true,
                ..offered
            }),
            Step::Inbox
        );
        assert_eq!(next(quiet(Seat::Peer)), Step::Idle);
        assert_eq!(
            next(Facts {
                own: true,
                ..quiet(Seat::Peer)
            }),
            Step::Inbox
        );
        assert_eq!(
            next(Facts {
                own: true,
                ..quiet(Seat::Main)
            }),
            Step::Return
        );
    }

    #[test]
    fn the_result_item_follows_a_closed_root() {
        let at = |root| Facts {
            root,
            ..quiet(Seat::Worker0)
        };
        assert_eq!(next(at(RootState::Main)), Step::Root(Root::Main));
        assert_eq!(next(at(RootState::Open)), Step::Idle);
        assert_eq!(next(at(RootState::Closed)), Step::Root(Root::Result));
        assert_eq!(next(at(RootState::Done)), Step::Return);
        assert_eq!(next(at(RootState::None)), Step::Return);
        let stopping = Facts {
            stop: true,
            ..at(RootState::Closed)
        };
        assert_eq!(next(stopping), Step::Stop);
        // 根はworker 0のものなので、ほかの席は根の状態を読まない
        let peer = Facts {
            root: RootState::Closed,
            ..quiet(Seat::Peer)
        };
        assert_eq!(next(peer), Step::Idle);
    }
}
