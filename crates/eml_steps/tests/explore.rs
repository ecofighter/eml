//! ループ、空き、判定のカーネルのexplorer (docs/implementation/testing.mdの「`eml_steps`の検証」)。出荷する`plan::next`
//! 、`idle::step`、`verdict::step`、`Open`、`Sleepers`を呼ぶシェルのモデルのすべてのインターリーブを列挙し、たどり着い
//! たすべての状態で表明 (1) から (5) を確かめる。幅優先の実行は、グラフ全体で (6) も確かめる。

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{BuildHasherDefault, DefaultHasher, Hash, Hasher};

use eml_steps::Label;
use eml_steps::open::Open;
use eml_steps::plan::{self, Facts, Root, RootState, Seat, Step};
use eml_steps::sleepers::Sleepers;
use eml_steps::{idle, verdict};

type IdleStep = fn(idle::Pc, idle::Seen) -> (idle::Pc, idle::Effect);
type VerdictStep = fn(verdict::Pc, verdict::Seen) -> (verdict::Pc, verdict::Effect);

#[derive(Clone, Copy, Debug)]
struct Bound {
    workers: usize,
    main: bool,
    publishes: u8,
    /// 項目が上げてよい源: `TIMER`、`OPS`、`WAITERS`、`HOLDS`、`ROOT` (根を閉じること)。
    sources: u8,
    virtual_clock: bool,
    stop: bool,
    /// 各publishの種類を前もって決める。`None`ならどちらでもよい。
    kinds: Option<[Kind; 3]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Kind {
    Join,
    Cell,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Body {
    Main,
    Task,
    Job(u8),
}

/// 流れている項目と、まだ済ませていないjoin。新しいものが後ろで、(仕事, 通し番号) の組で持つ。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Item {
    task: Body,
    joins: [(u8, u64); 3],
    depth: usize,
}

impl Item {
    fn pop(&mut self) {
        self.depth -= 1;
        self.joins[self.depth] = (0, 0);
    }
}

/// driverの待ちかメインのworkerの`Park`が、自分で戻った理由。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Source {
    Timer,
    Ops,
    Event,
    Holds,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Ctl {
    Loop,
    Item,
    /// 次に行う空きの効果と、その後のカーネルの状態。
    Kernel(idle::Pc, idle::Effect),
    /// 空きのカーネルの走査。ほかの欄の`start`番目から、1回の遷移で1つの欄を訪れる。
    Scan {
        pc: idle::Pc,
        start: usize,
        done: usize,
    },
    Foreign {
        start: usize,
        done: usize,
    },
    Sleep(idle::Pc),
    /// 自分で戻った待ちの後で`running`に数え直した。源は次の遷移で下げる。
    Woke(idle::Pc, Source),
    Verdict {
        pc: verdict::Pc,
        effect: verdict::Effect,
        idle: idle::Pc,
    },
    Blocked {
        pc: verdict::Pc,
        effect: verdict::Effect,
        idle: idle::Pc,
    },
    VerdictWoke {
        pc: verdict::Pc,
        idle: idle::Pc,
        source: Option<(Source, usize)>,
    },
    JoinWait(u8),
    Gone,
}

#[derive(Clone)]
struct W {
    ctl: Ctl,
    open: Open<u8>,
    sleeping: bool,
    woken: bool,
    places: u8,
    joined: u8,
    ready: u8,
    pass: u8,
    detached: bool,
    foreign: bool,
    looked: bool,
    own: bool,
    item: Option<Item>,
    taken: Option<u8>,
    pushing: Option<u8>,
    target: Option<u32>,
}

const TIMER: u8 = 1;
const OPS: u8 = 2;
const WAITERS: u8 = 4;
const HOLDS: u8 = 8;
const ROOT: u8 = 16;
const NONE: u8 = 0;
const EVERY: u8 = TIMER | OPS | WAITERS | HOLDS | ROOT;

#[derive(Clone)]
struct State {
    w: Vec<W>,
    set: Sleepers,
    running: u8,
    sleepers: u8,
    searching: u8,
    jobs: [(usize, Kind); 4],
    published: u8,
    timer: Option<usize>,
    ops: Option<usize>,
    waiters: bool,
    raised: u8,
    holds: bool,
    event: bool,
    mail: bool,
    notice: bool,
    relay: bool,
    root: RootState,
    closed: bool,
    stop: bool,
    stopped: bool,
    owed: u8,
}

fn nothing() -> idle::Seen {
    idle::Seen {
        probe: false,
        popped: false,
        took: false,
        quiet: false,
        zero: false,
        other: false,
    }
}

fn no_reads() -> verdict::Seen {
    verdict::Seen {
        others_running: false,
        listed: false,
        timer: false,
        ops: false,
        signal_waiters: false,
        root: verdict::RootFact::Open,
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

// 読むための関数を通してハッシュする。通し番号の数え手は、publishし積んだ仕事から決まるのでハッシュしない。`Open`に
// 足したフィールドは、ここに手で足す。
fn hash_open(open: &Open<u8>, h: &mut DefaultHasher) {
    let mut i = 0;
    while let Some(entry) = open.nth_join(i) {
        entry.hash(h);
        i += 1;
    }
    i.hash(h);
    let mut i = 0;
    while let Some(entry) = open.nth_cell(i) {
        entry.hash(h);
        i += 1;
    }
    i.hash(h);
}

impl State {
    fn new(b: &Bound) -> State {
        let slots = b.workers + usize::from(b.main);
        let w = W {
            ctl: Ctl::Loop,
            open: Open::new(),
            sleeping: false,
            woken: false,
            places: 0,
            joined: 0,
            ready: 0,
            pass: 0,
            detached: true,
            foreign: true,
            looked: true,
            own: false,
            item: None,
            taken: None,
            pushing: None,
            target: None,
        };
        State {
            w: vec![w; slots],
            set: Sleepers::new(),
            running: slots as u8,
            sleepers: 0,
            searching: 0,
            jobs: [(0, Kind::Join); 4],
            published: 0,
            timer: None,
            ops: None,
            waiters: false,
            raised: 0,
            holds: false,
            event: b.main,
            mail: false,
            notice: false,
            relay: false,
            root: RootState::Main,
            closed: false,
            stop: false,
            stopped: false,
            owed: 0,
        }
    }

    fn digest(&self) -> u128 {
        // パターンはすべてのフィールドを`..`なしで名指すので、後から足したフィールドは、ハッシュに入れるまでエラーか、
        // clippyが拒む使わない変数の警告になる。漏れたフィールドは、異なる状態を何の印もなく1つにしてしまう。
        let State {
            w,
            set,
            running,
            sleepers,
            searching,
            jobs,
            published,
            timer,
            ops,
            waiters,
            raised,
            holds,
            event,
            mail,
            notice,
            relay,
            root,
            closed,
            stop,
            stopped,
            owed,
        } = self;
        let mut halves = [0u64; 2];
        for (salt, half) in halves.iter_mut().enumerate() {
            let mut h = DefaultHasher::new();
            salt.hash(&mut h);
            for worker in w {
                let W {
                    ctl,
                    open,
                    sleeping,
                    woken,
                    places,
                    joined,
                    ready,
                    pass,
                    detached,
                    foreign,
                    looked,
                    own,
                    item,
                    taken,
                    pushing,
                    target,
                } = worker;
                ctl.hash(&mut h);
                hash_open(open, &mut h);
                (sleeping, woken, places, joined, ready, pass).hash(&mut h);
                (detached, foreign, looked, own).hash(&mut h);
                (item, taken, pushing, target).hash(&mut h);
            }
            let mut i = 0;
            while let Some(k) = set.nth(i) {
                k.hash(&mut h);
                i += 1;
            }
            i.hash(&mut h);
            (running, sleepers, searching).hash(&mut h);
            (jobs, published, timer, ops, waiters).hash(&mut h);
            (raised, holds, event, mail, notice).hash(&mut h);
            (relay, root, closed, stop, stopped, owed).hash(&mut h);
            *half = h.finish();
        }
        (u128::from(halves[0]) << 64) | u128::from(halves[1])
    }

    fn summary(&self) -> String {
        let mut out = String::new();
        for (k, w) in self.w.iter().enumerate() {
            out += &format!(
                "  w{k} {:?} sl={} wk={} open={} ready={} pl={} jn={} item={:?}\n",
                w.ctl,
                w.sleeping,
                w.woken,
                w.open.len(),
                w.ready,
                w.places,
                w.joined,
                w.item.map(|i| i.task)
            );
        }
        out += &format!(
            "  run={} slp={} srch={} timer={:?} ops={:?} wait={} holds={} mail={} relay={} root={:?} closed={} stop={} owed={}\n",
            self.running,
            self.sleepers,
            self.searching,
            self.timer,
            self.ops,
            self.waiters,
            self.holds,
            self.mail,
            self.relay,
            self.root,
            self.closed,
            self.stop,
            self.owed
        );
        out
    }

    fn count(&mut self) {
        self.running += 1;
    }

    fn uncount(&mut self) {
        self.running = self.running.checked_sub(1).expect("running underflows");
    }

    /// 起こしの受け渡し、`WakeZero`、中継、`wake_all`の起こし: 相手の欄の1つの区間。
    fn wake(&mut self, t: usize) {
        if self.w[t].ctl == Ctl::Gone {
            return;
        }
        self.w[t].woken = true;
        if self.w[t].sleeping {
            self.w[t].sleeping = false;
            self.count();
        }
    }

    /// 知らせの送り: 積むことと起こしの決定を、相手の欄の1つの区間で行う。
    fn notify(&mut self, t: usize, joined: Option<u8>) {
        match joined {
            Some(j) => self.w[t].joined |= 1 << j,
            None => self.w[t].places += 1,
        }
        if self.w[t].sleeping {
            self.w[t].woken = true;
            self.w[t].sleeping = false;
            self.count();
        }
    }

    /// 持ち主に渡す完了。driverを持つworkerならその場で、そうでなければ知らせとして渡す。
    fn deliver(&mut self, owner: usize, by: usize) {
        if owner == by {
            self.w[by].ready += 1;
        } else {
            self.notify(owner, None);
        }
    }

    fn leave(&mut self, k: usize) {
        if !self.w[k].sleeping {
            self.uncount();
        }
        let w = &mut self.w[k];
        w.sleeping = false;
        w.woken = false;
        w.ctl = Ctl::Gone;
        w.places = 0;
        w.joined = 0;
        w.ready = 0;
        w.pass = 0;
        w.item = None;
        w.taken = None;
        w.pushing = None;
        w.target = None;
        self.stop = true;
        for t in 0..self.w.len() {
            self.wake(t);
        }
    }

    fn inbox(&self, k: usize) -> bool {
        self.w[k].places > 0 || self.w[k].joined != 0
    }

    fn listed(&self) -> bool {
        self.w.iter().any(|w| w.open.len() > 0)
    }

    /// 走査する欄の次の欄から始めたときの順で並べた、ほかの欄。
    fn others(&self, k: usize) -> Vec<usize> {
        let n = self.w.len();
        (1..n).map(|d| (k + d) % n).collect()
    }

    fn all_asleep(&self) -> bool {
        self.w.iter().all(|w| w.sleeping || w.ctl == Ctl::Gone)
    }

    fn takers_asleep(&self, b: &Bound) -> bool {
        self.w[..b.workers]
            .iter()
            .all(|w| w.sleeping || w.ctl == Ctl::Gone)
    }

    /// 決めたがまだ行っていない起こし。worker 0への中継の起こしは、そのmailがまだ取られていない間だけ数える。worker 0が
    /// ループでmailを取った後は、その起こしは何も運ばない。
    fn in_flight(&self) -> bool {
        (self.relay && self.mail)
            || self.w.iter().any(|w| {
                matches!(
                    w.ctl,
                    Ctl::Kernel(
                        _,
                        idle::Effect::Probe
                            | idle::Effect::PassOn
                            | idle::Effect::Wake
                            | idle::Effect::WakeZero
                    )
                )
            })
    }

    fn sources_quiet(&self) -> bool {
        self.timer.is_none() && self.ops.is_none() && !self.waiters && !self.holds && !self.mail
    }

    /// 眠っているworkerの自分の待ちが、自分で戻ってよい源。
    fn own_source(&self, b: &Bound, k: usize) -> Option<Source> {
        let main = b.main && k == b.workers;
        if !b.virtual_clock && self.timer == Some(k) {
            Some(Source::Timer)
        } else if !b.virtual_clock && self.ops == Some(k) {
            Some(Source::Ops)
        } else if main && self.event {
            Some(Source::Event)
        } else if main && self.holds {
            Some(Source::Holds)
        } else {
            None
        }
    }

    fn lower(&mut self, k: usize, source: Source) {
        match source {
            Source::Timer => self.timer = None,
            Source::Ops => self.ops = None,
            Source::Event => self.event = false,
            Source::Holds => {}
        }
        self.w[k].ready += 1;
    }

    fn verdict_reads(&self, effect: verdict::Effect) -> verdict::Seen {
        let w = &self.w[0];
        match effect {
            verdict::Effect::Count => verdict::Seen {
                others_running: self.running != 0,
                ..no_reads()
            },
            verdict::Effect::Lists => verdict::Seen {
                listed: self.listed(),
                ..no_reads()
            },
            verdict::Effect::Registry => verdict::Seen {
                timer: self.timer.is_some(),
                ops: self.ops.is_some(),
                signal_waiters: self.waiters,
                root: if self.closed {
                    verdict::RootFact::Closed
                } else {
                    verdict::RootFact::Open
                },
                ..no_reads()
            },
            verdict::Effect::Own => verdict::Seen {
                inbox: self.inbox(0),
                woken: w.woken,
                others_running: self.running != 0,
                mail: self.mail,
                notice: self.notice,
                stop: self.stop,
                holds: self.holds,
                owed: self.owed != 0,
                ..no_reads()
            },
            _ => no_reads(),
        }
    }
}

#[derive(Default, Debug)]
struct Report {
    states: usize,
    labels: Vec<Label>,
}

impl Report {
    fn note(&mut self, label: Label) {
        if !self.labels.contains(&label) {
            self.labels.push(label);
        }
    }
}

struct Explorer<'a> {
    b: &'a Bound,
    idle_step: IdleStep,
    verdict_step: VerdictStep,
    report: Report,
    out: Vec<State>,
}

impl Explorer<'_> {
    fn kernel(&self, s: &mut State, k: usize, pc: idle::Pc, seen: idle::Seen) {
        let (pc, effect) = (self.idle_step)(pc, seen);
        s.w[k].ctl = Ctl::Kernel(pc, effect);
    }

    fn verdict(
        &self,
        s: &mut State,
        pc: verdict::Pc,
        seen: verdict::Seen,
        idle: idle::Pc,
    ) -> Result<(), String> {
        let (pc, effect) = (self.verdict_step)(pc, seen);
        if matches!(
            effect,
            verdict::Effect::RootClosed | verdict::Effect::Deadlock | verdict::Effect::Advance
        ) {
            self.check_end(s, effect)?;
        }
        s.w[0].ctl = Ctl::Verdict { pc, effect, idle };
        Ok(())
    }

    /// 表明 (2) と (4)。判定が終わりか時計を進めることを決めるところで確かめる。
    fn check_end(&self, s: &State, effect: verdict::Effect) -> Result<(), String> {
        let job_runs = s.w.iter().any(|w| {
            w.taken.is_some()
                || matches!(
                    w.item,
                    Some(Item {
                        task: Body::Job(_),
                        ..
                    })
                )
        });
        if s.listed()
            || job_runs
            || s.w
                .iter()
                .any(|w| w.joined != 0 || matches!(w.ctl, Ctl::JoinWait(_)))
        {
            return Err(format!("(2) {effect:?} with work left"));
        }
        if !s.all_asleep() || s.in_flight() {
            return Err(format!(
                "(4) {effect:?} with a worker running or a wake in flight"
            ));
        }
        if effect == verdict::Effect::Deadlock && !s.sources_quiet() {
            return Err("(4) Deadlock with a source raised".into());
        }
        Ok(())
    }

    fn push(&mut self, s: State) {
        self.out.push(s);
    }

    fn moves(&mut self, s: &State) -> Result<(), String> {
        for k in 0..s.w.len() {
            self.worker(s, k)?;
        }
        if s.waiters && !s.relay {
            let mut t = s.clone();
            t.waiters = false;
            t.mail = true;
            t.notice = true;
            t.relay = true;
            self.push(t);
        }
        if s.relay {
            let mut t = s.clone();
            t.relay = false;
            t.wake(0);
            self.push(t);
        }
        Ok(())
    }

    fn worker(&mut self, s: &State, k: usize) -> Result<(), String> {
        let b = self.b;
        let main = b.main && k == b.workers;
        let w = &s.w[k];
        match w.ctl {
            Ctl::Gone => {}
            Ctl::Loop => self.loop_step(s, k, main),
            Ctl::Item => self.item(s, k, main),
            Ctl::Kernel(pc, effect) => self.idle_effect(s, k, pc, effect)?,
            Ctl::Scan { pc, start, done } => {
                let mut t = s.clone();
                let others = s.others(k);
                let slot = others[(start + done) % others.len()];
                match t.w[slot].open.take_oldest() {
                    Some(j) => {
                        t.w[k].taken = Some(j);
                        self.kernel(
                            &mut t,
                            k,
                            pc,
                            idle::Seen {
                                took: true,
                                ..nothing()
                            },
                        );
                    }
                    None if done + 1 == others.len() => self.kernel(&mut t, k, pc, nothing()),
                    None => {
                        t.w[k].ctl = Ctl::Scan {
                            pc,
                            start,
                            done: done + 1,
                        }
                    }
                }
                self.push(t);
            }
            Ctl::Foreign { start, done } => {
                let mut t = s.clone();
                let others = s.others(k);
                let slot = others[(start + done) % others.len()];
                match t.w[slot].open.take_oldest() {
                    Some(j) => self.start_item(&mut t, k, Body::Job(j)),
                    None if done + 1 == others.len() => t.w[k].ctl = Ctl::Loop,
                    None => {
                        t.w[k].ctl = Ctl::Foreign {
                            start,
                            done: done + 1,
                        }
                    }
                }
                self.push(t);
            }
            Ctl::Sleep(pc) => {
                if !w.sleeping {
                    let mut t = s.clone();
                    t.w[k].woken = false;
                    let other = t.inbox(k) || t.stop;
                    self.kernel(&mut t, k, pc, idle::Seen { other, ..nothing() });
                    self.push(t);
                } else if let Some(source) = s.own_source(b, k) {
                    let mut t = s.clone();
                    t.w[k].sleeping = false;
                    t.count();
                    t.w[k].ctl = Ctl::Woke(pc, source);
                    self.push(t);
                }
            }
            Ctl::Woke(pc, source) => {
                let mut t = s.clone();
                t.lower(k, source);
                self.kernel(
                    &mut t,
                    k,
                    pc,
                    idle::Seen {
                        other: true,
                        ..nothing()
                    },
                );
                self.push(t);
            }
            Ctl::Verdict { pc, effect, idle } => self.verdict_effect(s, pc, effect, idle)?,
            Ctl::Blocked { pc, effect, idle } => {
                if effect == verdict::Effect::Advance {
                    let mut t = s.clone();
                    if t.w[0].sleeping {
                        t.w[0].sleeping = false;
                        t.count();
                    }
                    let source = match (s.timer, s.ops) {
                        (Some(o), _) => Some((Source::Timer, o)),
                        (None, Some(o)) => Some((Source::Ops, o)),
                        (None, None) => None,
                    };
                    t.w[0].ctl = Ctl::VerdictWoke { pc, idle, source };
                    self.push(t);
                } else if !w.sleeping {
                    let mut t = s.clone();
                    self.verdict(&mut t, pc, no_reads(), idle)?;
                    self.push(t);
                } else if effect == verdict::Effect::WaitUntil
                    && let Some(source) = s.own_source(b, 0)
                {
                    let mut t = s.clone();
                    t.w[0].sleeping = false;
                    t.count();
                    t.w[0].ctl = Ctl::VerdictWoke {
                        pc,
                        idle,
                        source: Some((source, 0)),
                    };
                    self.push(t);
                }
            }
            Ctl::VerdictWoke { pc, idle, source } => {
                let mut t = s.clone();
                if let Some((source, owner)) = source {
                    match source {
                        Source::Timer => t.timer = None,
                        Source::Ops => t.ops = None,
                        Source::Event | Source::Holds => unreachable!(),
                    }
                    t.deliver(owner, 0);
                }
                self.verdict(&mut t, pc, no_reads(), idle)?;
                self.push(t);
            }
            Ctl::JoinWait(j) => {
                if w.joined & (1 << j) != 0 {
                    let mut t = s.clone();
                    t.w[k].joined &= !(1 << j);
                    t.w[k].item.as_mut().unwrap().pop();
                    t.w[k].ctl = Ctl::Item;
                    self.push(t);
                } else if s.stop {
                    let mut t = s.clone();
                    t.leave(k);
                    self.push(t);
                }
            }
        }
        Ok(())
    }

    fn start_item(&self, t: &mut State, k: usize, task: Body) {
        t.w[k].item = Some(Item {
            task,
            joins: [(0, 0); 3],
            depth: 0,
        });
        t.w[k].ctl = Ctl::Item;
    }

    fn loop_step(&mut self, s: &State, k: usize, main: bool) {
        let w = &s.w[k];
        let seat = if k == 0 {
            Seat::Worker0
        } else if main {
            Seat::Main
        } else {
            Seat::Peer
        };
        let facts = Facts {
            seat,
            stop: s.stop,
            root: if k == 0 { s.root } else { RootState::Open },
            inbox: s.inbox(k) || (k == 0 && (s.mail || s.notice)),
            pass: u64::from(w.pass),
            ready_empty: w.ready == 0,
            own: w.own,
            detached: w.detached,
            foreign: w.foreign,
            looked: w.looked,
        };
        let step = plan::next(facts);
        self.report.note(Label::Loop(step));
        let mut t = s.clone();
        match step {
            Step::Stop => t.leave(k),
            Step::Inbox => {
                let w = &mut t.w[k];
                w.ready += w.places;
                w.places = 0;
                if k == 0 {
                    w.ready += u8::from(t.mail);
                    t.mail = false;
                    t.notice = false;
                }
                let w = &mut t.w[k];
                w.pass = w.ready;
                w.detached = false;
                w.foreign = false;
                w.looked = false;
            }
            Step::Root(Root::Main) => self.start_item(&mut t, k, Body::Main),
            Step::Root(Root::Result) => t.root = RootState::Done,
            Step::Ready => {
                t.w[k].pass -= 1;
                t.w[k].ready -= 1;
                self.start_item(&mut t, k, Body::Task);
            }
            Step::Detached => {
                t.w[k].detached = true;
                match t.w[k].open.take_detached() {
                    Some(j) => self.start_item(&mut t, k, Body::Job(j)),
                    None => t.w[k].own = false,
                }
            }
            Step::Foreign => {
                t.w[k].foreign = true;
                if s.w.len() == 1 {
                    self.push(t);
                    return;
                }
                for start in 0..s.w.len() - 1 {
                    let mut u = t.clone();
                    u.w[k].ctl = Ctl::Foreign { start, done: 0 };
                    self.push(u);
                }
                return;
            }
            Step::Look => {
                t.w[k].looked = true;
                if self.b.virtual_clock {
                    if k == 0
                        && let Some(owner) = t.ops
                    {
                        t.ops = None;
                        t.deliver(owner, 0);
                    }
                } else {
                    if t.timer == Some(k) {
                        t.lower(k, Source::Timer);
                    }
                    if t.ops == Some(k) {
                        t.lower(k, Source::Ops);
                    }
                }
            }
            Step::Idle => {
                let pc = idle::Pc {
                    at: idle::At::Idle,
                    worker_0: k == 0,
                };
                self.kernel(&mut t, k, pc, nothing());
            }
            Step::Return if k == 0 => t.leave(0),
            Step::Return => {
                let pc = idle::Pc {
                    at: idle::At::MainPark,
                    worker_0: false,
                };
                self.kernel(&mut t, k, pc, nothing());
            }
        }
        self.push(t);
    }

    fn item(&mut self, s: &State, k: usize, main: bool) {
        let b = self.b;
        let item = s.w[k].item.unwrap();
        if s.published < b.publishes {
            let kinds = match b.kinds {
                Some(kinds) => vec![kinds[usize::from(s.published)]],
                None => vec![Kind::Join, Kind::Cell],
            };
            for kind in kinds {
                let mut t = s.clone();
                let j = t.published;
                t.jobs[usize::from(j)] = (k, kind);
                t.published += 1;
                t.w[k].pushing = Some(j);
                let pc = idle::Pc {
                    at: idle::At::Publish,
                    worker_0: k == 0,
                };
                self.kernel(&mut t, k, pc, nothing());
                self.push(t);
            }
        }
        for (bit, raise) in [(TIMER, 0), (OPS, 1), (WAITERS, 2)] {
            if s.raised & bit == 0 && b.sources & bit != 0 {
                let mut t = s.clone();
                t.raised |= bit;
                match raise {
                    0 => t.timer = Some(k),
                    1 => t.ops = Some(k),
                    _ => t.waiters = true,
                }
                self.push(t);
            }
        }
        if s.root == RootState::Open && !s.closed && b.sources & ROOT != 0 {
            let mut t = s.clone();
            t.closed = true;
            self.push(t);
        }
        if main && s.raised & HOLDS == 0 && b.sources & HOLDS != 0 {
            let mut t = s.clone();
            t.raised |= HOLDS;
            t.holds = true;
            self.push(t);
        }
        if main && s.holds {
            let mut t = s.clone();
            t.holds = false;
            t.wake(0);
            self.push(t);
        }
        if b.stop && !s.stopped && s.w[k].open.len() > 0 {
            let mut t = s.clone();
            t.stopped = true;
            t.leave(k);
            self.push(t);
        }
        let mut t = s.clone();
        if item.depth > 0 {
            let (j, serial) = item.joins[item.depth - 1];
            match t.w[k].open.take_own(serial) {
                Some(taken) => {
                    assert_eq!(taken, j);
                    t.w[k].item.as_mut().unwrap().pop();
                }
                None => t.w[k].ctl = Ctl::JoinWait(j),
            }
        } else {
            match item.task {
                Body::Job(j) if t.jobs[usize::from(j)].1 == Kind::Join => {
                    t.notify(t.jobs[usize::from(j)].0, Some(j));
                }
                Body::Main => t.root = RootState::Open,
                _ => {}
            }
            t.w[k].item = None;
            t.w[k].ctl = Ctl::Loop;
        }
        self.push(t);
    }

    fn idle_effect(
        &mut self,
        s: &State,
        k: usize,
        pc: idle::Pc,
        effect: idle::Effect,
    ) -> Result<(), String> {
        self.report.note(Label::Idle(effect));
        let mut t = s.clone();
        let mut seen = nothing();
        match effect {
            idle::Effect::Push => {
                let j = t.w[k].pushing.take().unwrap();
                match t.jobs[usize::from(j)].1 {
                    Kind::Join => {
                        let serial = t.w[k].open.push_join(j).unwrap();
                        let item = t.w[k].item.as_mut().unwrap();
                        item.joins[item.depth] = (j, serial);
                        item.depth += 1;
                    }
                    Kind::Cell => {
                        t.w[k].open.push_cell(j).unwrap();
                        t.w[k].own = true;
                    }
                }
            }
            idle::Effect::Probe => seen.probe = t.sleepers > 0 && t.searching == 0,
            idle::Effect::PassOn => {
                t.w[k].target = t.set.pop();
                seen.popped = t.w[k].target.is_some();
            }
            idle::Effect::Wake => {
                let target = t.w[k].target.take().unwrap();
                t.wake(target as usize);
            }
            idle::Effect::IncSleepers => t.sleepers += 1,
            idle::Effect::DecSleepers => {
                t.sleepers = t.sleepers.checked_sub(1).expect("sleepers underflows");
            }
            idle::Effect::IncSearching => t.searching += 1,
            idle::Effect::DecSearching => {
                t.searching = t.searching.checked_sub(1).expect("searching underflows");
            }
            idle::Effect::Enlist => {
                t.set.enlist(k as u32);
            }
            idle::Effect::Delist => {
                t.set.delist(k as u32);
            }
            idle::Effect::Scan => {
                if s.w.len() == 1 {
                    self.kernel(&mut t, k, pc, nothing());
                    self.push(t);
                    return Ok(());
                }
                for start in 0..s.w.len() - 1 {
                    let mut u = t.clone();
                    u.w[k].ctl = Ctl::Scan { pc, start, done: 0 };
                    self.push(u);
                }
                return Ok(());
            }
            // モデルは区間を1つのアトミックな遷移として扱う。区間の中のシェルの順番はkernelの説明にある
            idle::Effect::Park => {
                if t.w[k].woken || t.inbox(k) || t.stop {
                    t.w[k].woken = false;
                    seen.other = t.inbox(k) || t.stop;
                } else {
                    t.w[k].sleeping = true;
                    t.uncount();
                    seen.quiet = true;
                    seen.zero = t.running == 0;
                    if seen.zero && k != 0 {
                        t.owed += 1;
                    }
                }
            }
            idle::Effect::WakeZero => {
                t.wake(0);
                t.owed = t.owed.checked_sub(1).expect("owed underflows");
            }
            idle::Effect::Sleep if k == 0 => {
                let start = verdict::Pc {
                    at: verdict::At::Start,
                    virtual_clock: self.b.virtual_clock,
                    stalled: false,
                };
                self.verdict(&mut t, start, no_reads(), pc)?;
                self.push(t);
                return Ok(());
            }
            idle::Effect::Sleep => {
                t.w[k].ctl = Ctl::Sleep(pc);
                self.push(t);
                return Ok(());
            }
            idle::Effect::Work => {
                t.w[k].ctl = if let Some(j) = t.w[k].taken.take() {
                    self.start_item(&mut t, k, Body::Job(j));
                    Ctl::Item
                } else if t.w[k].item.is_some() {
                    Ctl::Item
                } else {
                    Ctl::Loop
                };
                self.push(t);
                return Ok(());
            }
        }
        self.kernel(&mut t, k, pc, seen);
        self.push(t);
        Ok(())
    }

    fn verdict_effect(
        &mut self,
        s: &State,
        pc: verdict::Pc,
        effect: verdict::Effect,
        idle: idle::Pc,
    ) -> Result<(), String> {
        self.report.note(Label::Verdict(effect));
        let mut t = s.clone();
        match effect {
            verdict::Effect::Count
            | verdict::Effect::Lists
            | verdict::Effect::Registry
            | verdict::Effect::Own => {
                let seen = s.verdict_reads(effect);
                self.verdict(&mut t, pc, seen, idle)?;
            }
            verdict::Effect::Wait | verdict::Effect::WaitUntil | verdict::Effect::Advance => {
                t.w[0].ctl = Ctl::Blocked { pc, effect, idle };
            }
            verdict::Effect::Restart | verdict::Effect::RootClosed => {
                if t.w[0].sleeping {
                    t.w[0].sleeping = false;
                    t.count();
                }
                t.w[0].woken = false;
                if effect == verdict::Effect::RootClosed {
                    t.root = RootState::Closed;
                }
                self.kernel(
                    &mut t,
                    0,
                    idle,
                    idle::Seen {
                        other: true,
                        ..nothing()
                    },
                );
            }
            verdict::Effect::Deadlock | verdict::Effect::Stop => t.leave(0),
        }
        self.push(t);
        Ok(())
    }
}

/// 表明 (1)。終わっていないrunのすべての状態で確かめる。
fn check_state(s: &State, b: &Bound) -> Result<(), String> {
    if !s.stop && s.listed() && s.takers_asleep(b) && !s.in_flight() {
        return Err("(1) a listed job with every taker asleep and no wake in flight".into());
    }
    Ok(())
}

/// 表明 (5): 根が閉じてすべてのworkerが眠った状態から、worker 0の判定を単独で、`stalled`を下ろしても立てても流すと、源が
/// 何であっても`RootClosed`で終わる。
fn check_root(s: &State, b: &Bound, verdict_step: VerdictStep) -> Result<(), String> {
    let in_verdict = matches!(s.w[0].ctl, Ctl::Verdict { .. } | Ctl::Blocked { .. });
    let own_quiet = !s.inbox(0) && !s.w[0].woken && !s.mail && !s.notice && s.owed == 0;
    if !(s.closed && !s.stop && in_verdict && s.all_asleep() && !s.in_flight() && own_quiet) {
        return Ok(());
    }
    if s.listed() {
        return Err("(5) a listed job with the root closed and every worker asleep".into());
    }
    for stalled in [false, true] {
        let mut pc = verdict::Pc {
            at: verdict::At::Start,
            virtual_clock: b.virtual_clock,
            stalled,
        };
        let mut seen = no_reads();
        loop {
            let (next, effect) = verdict_step(pc, seen);
            match effect {
                verdict::Effect::RootClosed => break,
                verdict::Effect::Count
                | verdict::Effect::Lists
                | verdict::Effect::Registry
                | verdict::Effect::Own => seen = s.verdict_reads(effect),
                other => return Err(format!("(5) the verdict alone ends in {other:?}")),
            }
            pc = next;
        }
    }
    Ok(())
}

/// digestはもう一様なので、集合はその下位のビットをハッシュに使う。
#[derive(Default)]
struct Low(u64);

impl Hasher for Low {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, _: &[u8]) {
        unreachable!()
    }

    fn write_u128(&mut self, n: u128) {
        self.0 = n as u64;
    }
}

/// 訪れた状態の集合。部分の伸びで全体の記憶が2倍にならないよう、256の部分に分ける。部分の大きさを1オクターブの幅に
/// 散らすので、部分が表を広げる時がそろわない。状態が飛ばされるのは、その128ビットのdigestがほかの状態と等しいときだけである。
struct Visited {
    parts: Vec<HashSet<u128, BuildHasherDefault<Low>>>,
    /// i番目の部分が受け持つdigestの上位16ビットの上端。部分iの重みは2^(i/256)である。
    ends: Vec<u32>,
    len: usize,
}

impl Visited {
    fn new() -> Visited {
        let weights: Vec<f64> = (0..256).map(|i| (f64::from(i) / 256.0).exp2()).collect();
        let total: f64 = weights.iter().sum();
        let mut sum = 0.0;
        let ends = weights
            .iter()
            .map(|w| {
                sum += w;
                (sum / total * 65536.0).round() as u32
            })
            .collect();
        Visited {
            parts: (0..256).map(|_| HashSet::default()).collect(),
            ends,
            len: 0,
        }
    }

    fn insert(&mut self, s: &State) -> bool {
        let d = s.digest();
        let top = (d >> 112) as u32;
        let part = self.ends.partition_point(|&end| end <= top);
        let fresh = self.parts[part].insert(d);
        self.len += usize::from(fresh);
        fresh
    }
}

#[derive(Clone, Copy)]
enum Order {
    /// 幅優先。すべての状態とその辺を持ち、最後に (6) を確かめ、失敗のときにはその状態までの最短の道を出す。
    Breadth,
    /// 深さ優先。digestとスタックだけを持ち、大きな境界に使う。
    Depth,
}

fn explore(
    b: Bound,
    idle_step: IdleStep,
    verdict_step: VerdictStep,
    order: Order,
) -> Result<Report, String> {
    let mut x = Explorer {
        b: &b,
        idle_step,
        verdict_step,
        report: Report::default(),
        out: Vec::new(),
    };
    let start = State::new(&b);
    let mut visited = Visited::new();
    // 幅優先では状態の番号をdigestから引き、辺を (元, 先) の組で持つ。
    let mut index: HashMap<u128, u32, BuildHasherDefault<Low>> = HashMap::default();
    let mut edges: Vec<(u32, u32)> = Vec::new();
    match order {
        Order::Breadth => {
            index.insert(start.digest(), 0);
        }
        Order::Depth => {
            visited.insert(&start);
        }
    }
    // 幅優先では各状態を親の番号とともに持ち、深さ優先ではスタックだけを持つ。
    let mut all: Vec<(State, usize)> = Vec::new();
    let mut queue: VecDeque<usize> = VecDeque::new();
    let mut stack: Vec<State> = Vec::new();
    match order {
        Order::Breadth => {
            all.push((start, usize::MAX));
            queue.push_back(0);
        }
        Order::Depth => stack.push(start),
    }
    let path = |all: &Vec<(State, usize)>, mut i: usize| -> String {
        let mut lines = Vec::new();
        while i != usize::MAX {
            lines.push(all[i].0.summary());
            i = all[i].1;
        }
        lines.reverse();
        lines.join("\n")
    };
    loop {
        let (s, at) = match order {
            Order::Breadth => match queue.pop_front() {
                Some(i) => (all[i].0.clone(), i),
                None => break,
            },
            Order::Depth => match stack.pop() {
                Some(s) => (s, usize::MAX),
                None => break,
            },
        };
        let shown = |all: &Vec<(State, usize)>, s: &State| match order {
            Order::Breadth => path(all, at),
            Order::Depth => s.summary(),
        };
        let fail = |e: String, all: &Vec<(State, usize)>, s: &State| {
            format!("{b:?}: {e}\n{}", shown(all, s))
        };
        check_root(&s, &b, verdict_step).map_err(|e| fail(e, &all, &s))?;
        x.out.clear();
        x.moves(&s).map_err(|e| fail(e, &all, &s))?;
        if x.out.is_empty() && s.w.iter().any(|w| w.ctl != Ctl::Gone) {
            return Err(fail(
                "(3) a stuck state that is not the end of the run".into(),
                &all,
                &s,
            ));
        }
        for t in std::mem::take(&mut x.out) {
            if let Err(e) = check_state(&t, &b) {
                return Err(format!("{}\n{}", fail(e, &all, &s), t.summary()));
            }
            match order {
                Order::Breadth => {
                    let fresh = all.len() as u32;
                    let to = *index.entry(t.digest()).or_insert(fresh);
                    if to == fresh {
                        all.push((t, at));
                        queue.push_back(all.len() - 1);
                    }
                    edges.push((at as u32, to));
                }
                Order::Depth => {
                    if visited.insert(&t) {
                        stack.push(t);
                    }
                }
            }
        }
    }
    match order {
        Order::Breadth => {
            if let Some(i) = cannot_end(&all, edges) {
                return Err(format!(
                    "{b:?}: (6) the run cannot end from a reached state\n{}",
                    path(&all, i)
                ));
            }
            x.report.states = all.len();
        }
        Order::Depth => x.report.states = visited.len,
    }
    Ok(x.report)
}

/// 表明 (6): たどり着いたすべての状態から、すべてのworkerが抜けた状態にたどり着ける。終わりの状態から記録した辺を逆にたど
/// って印を付け、印のない最初の状態を番号の順で返す。
fn cannot_end(all: &[(State, usize)], mut edges: Vec<(u32, u32)>) -> Option<usize> {
    edges.sort_unstable_by_key(|&(from, to)| (to, from));
    edges.dedup();
    let mut first = vec![0usize; all.len() + 1];
    for &(_, to) in &edges {
        first[to as usize + 1] += 1;
    }
    for i in 0..all.len() {
        first[i + 1] += first[i];
    }
    let mut marked = vec![false; all.len()];
    let mut work: Vec<usize> = Vec::new();
    for (i, (s, _)) in all.iter().enumerate() {
        if s.w.iter().all(|w| w.ctl == Ctl::Gone) {
            marked[i] = true;
            work.push(i);
        }
    }
    while let Some(to) = work.pop() {
        for &(from, _) in &edges[first[to]..first[to + 1]] {
            if !marked[from as usize] {
                marked[from as usize] = true;
                work.push(from as usize);
            }
        }
    }
    marked.iter().position(|&m| !m)
}

fn bound(workers: usize, main: bool, publishes: u8, sources: u8) -> Bound {
    Bound {
        workers,
        main,
        publishes,
        sources,
        virtual_clock: true,
        stop: false,
        kinds: None,
    }
}

/// 出荷するカーネルの上で、境界を仮想の時計と本物の時計で幅優先に流したときの状態の数。
fn both_clocks(b: Bound) -> [usize; 2] {
    [true, false].map(|virtual_clock| breadth(Bound { virtual_clock, ..b }))
}

#[test]
fn two_workers_two_publishes() {
    assert_eq!(both_clocks(bound(2, false, 2, NONE)), [25_367, 26_454]);
}

fn breadth(b: Bound) -> usize {
    explore(b, idle::step, verdict::step, Order::Breadth)
        .unwrap()
        .states
}

#[test]
fn two_workers_with_every_source_virtual() {
    assert_eq!(breadth(bound(2, false, 0, EVERY)), 77_611);
}

#[test]
fn two_workers_with_every_source_real() {
    let b = Bound {
        virtual_clock: false,
        ..bound(2, false, 0, EVERY)
    };
    assert_eq!(breadth(b), 78_493);
}

#[test]
fn the_main_worker_parks_and_returns() {
    assert_eq!(both_clocks(bound(2, true, 0, NONE)), [26_411, 27_555]);
}

#[test]
fn a_stopped_worker_with_a_listed_job() {
    let b = Bound {
        stop: true,
        ..bound(2, false, 2, NONE)
    };
    assert_eq!(both_clocks(b), [25_955, 27_044]);
}

#[test]
fn the_labels_are_the_kernel_alphabet() {
    let runs = [
        bound(2, false, 1, NONE),
        Bound {
            virtual_clock: false,
            ..bound(2, false, 1, NONE)
        },
        bound(2, false, 0, TIMER | ROOT),
        Bound {
            stop: true,
            ..bound(1, true, 1, NONE)
        },
    ];
    let mut labels: Vec<Label> = Vec::new();
    let mut counts = Vec::new();
    for b in runs {
        let report = explore(b, idle::step, verdict::step, Order::Breadth).unwrap();
        counts.push(report.states);
        for label in report.labels {
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
    }
    let missing: Vec<&Label> = Label::ALL.iter().filter(|l| !labels.contains(l)).collect();
    assert_eq!(missing, Vec::<&Label>::new());
    assert_eq!(labels.len(), Label::ALL.len());
    assert_eq!(counts, [3_893, 4_099, 3_226, 4_426]);
}

/// 出す側の`Probe`を忘れるカーネルのシェル。publishしても起こしを渡さない。
fn without_probe(pc: idle::Pc, seen: idle::Seen) -> (idle::Pc, idle::Effect) {
    let (next, effect) = idle::step(pc, seen);
    if effect == idle::Effect::Probe {
        idle::step(next, nothing())
    } else {
        (next, effect)
    }
}

#[test]
fn a_dropped_probe_is_found() {
    // ほかの出す側はどれも眠る前に自分の仕事を取り戻すので、`Probe`に頼るのはメインのworkerのpublishだけである。
    let b = bound(1, true, 1, NONE);
    let shipped = explore(b, idle::step, verdict::step, Order::Breadth).unwrap();
    assert_eq!(shipped.states, 4_284);
    let found = explore(b, without_probe, verdict::step, Order::Breadth).unwrap_err();
    assert!(found.contains("(1)"), "{found}");
}

/// `owed`を読まない判定のシェル。
fn without_owed(pc: verdict::Pc, seen: verdict::Seen) -> (verdict::Pc, verdict::Effect) {
    verdict::step(
        pc,
        verdict::Seen {
            owed: false,
            ..seen
        },
    )
}

#[test]
fn an_ignored_owed_is_found() {
    let found = explore(
        bound(2, false, 0, NONE),
        idle::step,
        without_owed,
        Order::Breadth,
    )
    .unwrap_err();
    assert!(found.contains("(4)"), "{found}");
}

/// kernelが`WaitUntil`で待つところでループに戻る判定のシェル。
fn never_waits(pc: verdict::Pc, seen: verdict::Seen) -> (verdict::Pc, verdict::Effect) {
    let (next, effect) = verdict::step(pc, seen);
    if effect == verdict::Effect::WaitUntil {
        (
            verdict::Pc {
                at: verdict::At::End,
                ..next
            },
            verdict::Effect::Restart,
        )
    } else {
        (next, effect)
    }
}

#[test]
fn a_verdict_that_never_waits_is_found() {
    let b = Bound {
        virtual_clock: false,
        ..bound(2, false, 0, TIMER)
    };
    let shipped = explore(b, idle::step, verdict::step, Order::Breadth).unwrap();
    assert_eq!(shipped.states, 3_019);
    let found = explore(b, idle::step, never_waits, Order::Breadth).unwrap_err();
    assert!(found.contains("(6)"), "{found}");
}

#[test]
fn the_main_worker_holds() {
    assert_eq!(
        both_clocks(bound(1, true, 0, HOLDS | ROOT | TIMER)),
        [13_545, 11_848]
    );
}

/// 網羅のテストの行を1つの時計で深さ優先に流す。`first`があれば、各行を`first`で始まるpublishの種類の列ごとに、それぞれ
/// 別の訪れた状態の集合で1回ずつ流す。両方の始めの種類の列を合わせたものが、種類を決めない行である。各回の状態の数を順
/// に返す。
fn exhaustive(rows: &[Bound], virtual_clock: bool, first: Option<Kind>) -> Vec<usize> {
    let mut counts = Vec::new();
    for &row in rows {
        let row = Bound {
            virtual_clock,
            ..row
        };
        let sequences: Vec<Option<[Kind; 3]>> = match first {
            None => vec![None],
            Some(first) => (0..1u8 << (row.publishes - 1))
                .map(|bits| {
                    Some([0, 1, 2].map(|i| match i {
                        0 => first,
                        _ if bits >> (i - 1) & 1 == 0 => Kind::Join,
                        _ => Kind::Cell,
                    }))
                })
                .collect(),
        };
        for kinds in sequences {
            let b = Bound { kinds, ..row };
            match explore(b, idle::step, verdict::step, Order::Depth) {
                Ok(report) => counts.push(report.states),
                Err(e) => panic!("{e}"),
            }
        }
    }
    counts
}

fn stopping(b: Bound) -> Bound {
    Bound { stop: true, ..b }
}

fn three_by_three() -> Vec<Bound> {
    vec![bound(3, false, 3, NONE)]
}

fn three_by_three_stopping() -> Vec<Bound> {
    vec![stopping(bound(3, false, 3, NONE))]
}

fn main_publishes() -> Vec<Bound> {
    vec![
        bound(2, true, 3, NONE),
        stopping(bound(2, true, 3, NONE)),
        bound(3, true, 1, NONE),
    ]
}

fn two_workers_every_source() -> Vec<Bound> {
    vec![
        bound(2, false, 3, EVERY),
        stopping(bound(2, false, 2, EVERY)),
    ]
}

fn three_workers_every_source() -> Vec<Bound> {
    vec![bound(3, false, 1, EVERY)]
}

fn main_with_every_source() -> Vec<Bound> {
    vec![
        bound(2, true, 0, EVERY),
        bound(2, true, 2, HOLDS | ROOT),
        bound(2, true, 1, HOLDS | ROOT | TIMER),
        bound(2, true, 1, HOLDS | ROOT | OPS),
        bound(2, true, 1, HOLDS | ROOT | WAITERS),
    ]
}

/// 中くらいの境界を幅優先に流し、publishがすべての源と出会うところでも (6) を確かめる。
#[test]
#[ignore = "exhaustive"]
fn exhaustive_the_end_stays_reachable() {
    assert_eq!(both_clocks(bound(2, false, 1, EVERY)), [986_642, 1_144_648]);
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_three_workers_three_publishes_virtual() {
    assert_eq!(exhaustive(&three_by_three(), true, None), [46_469_811]);
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_three_workers_three_publishes_real() {
    assert_eq!(exhaustive(&three_by_three(), false, None), [48_063_507]);
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_a_worker_stops_with_a_listed_job_virtual() {
    assert_eq!(
        exhaustive(&three_by_three_stopping(), true, None),
        [47_057_583]
    );
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_a_worker_stops_with_a_listed_job_real() {
    assert_eq!(
        exhaustive(&three_by_three_stopping(), false, None),
        [48_656_625]
    );
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_the_main_worker_publishes_virtual_join_first() {
    assert_eq!(
        exhaustive(&main_publishes(), true, Some(Kind::Join)),
        [
            3_403_365, 4_056_529, 4_012_878, 4_767_495, 3_547_683, 4_217_904, 4_151_527, 4_923_029,
            19_693_181
        ]
    );
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_the_main_worker_publishes_virtual_cell_first() {
    assert_eq!(
        exhaustive(&main_publishes(), true, Some(Kind::Cell)),
        [
            3_508_410, 4_188_535, 4_156_665, 4_881_324, 3_687_612, 4_384_526, 4_326_225, 5_066_549,
            22_780_006
        ]
    );
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_the_main_worker_publishes_real_join_first() {
    assert_eq!(
        exhaustive(&main_publishes(), false, Some(Kind::Join)),
        [
            3_518_117, 4_184_188, 4_140_373, 4_907_705, 3_664_809, 4_347_977, 4_281_173, 5_065_479,
            20_407_538
        ]
    );
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_the_main_worker_publishes_real_cell_first() {
    assert_eq!(
        exhaustive(&main_publishes(), false, Some(Kind::Cell)),
        [
            3_620_797, 4_313_207, 4_281_355, 5_017_326, 3_802_690, 4_511_878, 4_453_350, 5_205_023,
            23_569_991
        ]
    );
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_two_workers_every_source_virtual() {
    assert_eq!(
        exhaustive(&two_workers_every_source(), true, None),
        [54_421_327, 7_924_670]
    );
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_two_workers_every_source_real() {
    assert_eq!(
        exhaustive(&two_workers_every_source(), false, None),
        [58_939_676, 8_623_750]
    );
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_three_workers_every_source_virtual_join_first() {
    assert_eq!(
        exhaustive(&three_workers_every_source(), true, Some(Kind::Join)),
        [41_998_653]
    );
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_three_workers_every_source_virtual_cell_first() {
    assert_eq!(
        exhaustive(&three_workers_every_source(), true, Some(Kind::Cell)),
        [58_946_031]
    );
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_three_workers_every_source_real_join_first() {
    assert_eq!(
        exhaustive(&three_workers_every_source(), false, Some(Kind::Join)),
        [62_893_255]
    );
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_three_workers_every_source_real_cell_first() {
    assert_eq!(
        exhaustive(&three_workers_every_source(), false, Some(Kind::Cell)),
        [67_014_739]
    );
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_the_main_worker_with_every_source_virtual() {
    assert_eq!(
        exhaustive(&main_with_every_source(), true, None),
        [14_054_254, 17_878_000, 9_184_650, 9_189_397, 11_740_644]
    );
}

#[test]
#[ignore = "exhaustive"]
fn exhaustive_the_main_worker_with_every_source_real() {
    assert_eq!(
        exhaustive(&main_with_every_source(), false, None),
        [9_419_516, 18_501_656, 9_184_959, 9_184_959, 12_156_970]
    );
}
