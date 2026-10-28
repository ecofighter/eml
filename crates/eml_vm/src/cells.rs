//! `Async` のタスクとスコープと channel の意味の核 (docs/spec/vm.md の「`Async` のタスク」、「channel」)。VM の
//! `Task`、`Scheduler`、フレームを知らず、走らせる仕事を `Runnable` と不透明な `WaitToken` で返す。ネイティブのコードが
//! 同じ核を `extern "C"` の入口から呼べるように、`std` と `eml_rt` だけに依存する。
//!
//! セルが持つ語は数えた参照で、VM が1回入れて1回出す。`Registry` はヒープに触れる `Drop` を持たないので、誤りで終わった
//! run と捨てた run では、`Scheduler` と一緒に触れずに捨てる。
//!
//! 不変条件: スコープの `members` と `nested` の項は、いつも生きているセルを指す。`waiters` の項は古くてもよく、
//! `wake_waiter` が `wait` で確かめる。`k.is_some()` と、`wait` が `Some(Cell(_))` か `Some(Time(_))` であることは
//! 同値である (`Ready` の継続は列に積んだ仕事が持つ)。`timers` の項 `key` と `wait == Some(Time(key))` は、いつも両方
//! あるか両方ないかである。タスクの今のスコープ `current` から親へたどる列は、そのタスクが開いたスコープだけからなり、加わったスコープ `scope` で終わる。入れ子の `run` の根の
//! `opener` は呼んだタスクで、根は誰の今のスコープにもならず、放されるまで呼んだタスクの `run` が名指す。`closed` の
//! スコープは `closing` で、`members` が空である。スコープのセルを放すのは、閉じた後の `release_scope`
//! (`Take::Run` は根の待ちが `closed` で起きた後に、`leave_scope` は今のスコープを親に戻した後に呼ぶ) と、run の終わりの
//! `sweep_all` だけである。
//! タスクのセルを放すのは、ハンドルを持つ側の `take`、自分のハンドルを落とした知らせの処理、`sweep_all` の3つだけで、
//! 待つ側はハンドルを持つので、待つ相手のセルはその `take` の前に放されない。
//!
//! channel の不変条件: タスクが `senders` か `receivers` にあるのは、そのタスクが止まっていて
//! `wait == Some(Cell(その channel))` のときだけで、1回だけ現れる。`closed` の後、両方の列はいつも空である。
//! channel のセルを放すのは `settle` と `sweep_all` だけである。品物があるのに受け手が待っている状態は、受け手を起こす
//! 仕事が実行待ちにあるときにだけありうる。積めるのに送り手が待っている状態も同じである。起こす側は枠も品物も取って
//! おかないので、起こされたタスクが速い道をやり直す (docs/spec/vm.md の「channel」)。
//!
//! 取り消しは、印を付ける `mark` と受け箱の処理 `drain` が待ちを外す `unpark` の2段で行う。`CANCELLED` で再開する
//! 仕事をほかのタスクのために作るのは `drain` だけである。外からの印付けは `shield` の境目 `boundary` で止まる。
//! `cancel`、`flag`、`cancel_new` はどのタスクの操作からも書く。`current` と `run` は持ち主だけが書き、印を付ける
//! 操作が読む。スコープの `shield` と `boundary` は開くときに1回書き、後は読むだけである。タスクのセルの残りは、
//! そのタスクを動かすか受け箱を処理する持ち主だけが読み書きする (docs/spec/vm.md の「取り消しの届け方」)。

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use eml_rt::{Control, Ctx, Fault, HeapError, Mode, Word};

/// 再開するタスクに渡す `Std.Async.Wake` の `Woke` のタグ (docs/spec/stdlib.md の「`Std.Async`」)。
pub(crate) const WOKE: i64 = 0;
/// `Cancelled` のタグ。
pub(crate) const CANCELLED: i64 = 1;
/// `IOErrorKind` の `Cancelled` のタグ。この種類で終わったタスクは失敗に数えない。
pub(crate) const IO_CANCELLED: i64 = 5;
/// 待たずに済む速い道の予算 (docs/spec/stdlib.md の「操作の予算」)。
const BUDGET: u32 = 128;

/// 登録表のセルの番号。添字を使い回すので、generation で古い番号を見分ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellRef {
    index: u32,
    generation: u32,
}

impl CellRef {
    /// `Std.Async.Cell` の即値 `(generation << 32) | index` (docs/spec/stdlib.md の「`Std.Async`」)。
    fn encode(self) -> i64 {
        (i64::from(self.generation) << 32) | i64::from(self.index)
    }

    fn decode(c: i64) -> CellRef {
        CellRef {
            index: c as u32,
            generation: (c >> 32) as u32,
        }
    }
}

/// token で待つものの番号。実行系が自分の単位 (VM では `TaskId`) に写す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct WaitToken(pub u64);

/// 核が実行系に渡す仕事。
pub(crate) enum Runnable {
    /// タスク `cell` を始める。`closure` は入口 `start_task` に渡す閉包で、仕事が参照を1つ持つ。
    Start { cell: CellRef, closure: Word },
    /// 待っていたタスク `cell` を、継続 `k` に `wake` を渡して再開する。`k` は仕事が参照を1つ持つ。
    Resume { cell: CellRef, k: Word, wake: i64 },
    /// token で待つものを起こす。
    Token(WaitToken),
}

/// セルが終わるか閉じるのを待つもの。
enum Waiter {
    /// 継続だけになって待つタスク。継続は待つタスクのセルが持つ。
    Task(CellRef),
    /// スタックを持ったまま待つもの。`Async.run` を呼んだ VM の `Task` である。
    Token(WaitToken),
}

/// タスクの終わり方。
pub(crate) enum Outcome {
    /// `finish_ok` が預けた語そのもの。即値のこともある。ランタイムは写さず中身を見ない。
    Value(Word),
    /// IO の失敗。文言は Rust の文字列で持つので、失敗の語は持ち主をまたがない。
    Io {
        kind: i64,
        message: String,
    },
    Panic(String),
}

impl Outcome {
    /// 失敗なら、スコープに記録する写し。
    fn failure(&self) -> Option<Outcome> {
        match self {
            Outcome::Value(_) => None,
            Outcome::Io { kind, .. } if *kind == IO_CANCELLED => None,
            Outcome::Io { kind, message } => Some(Outcome::Io {
                kind: *kind,
                message: message.clone(),
            }),
            Outcome::Panic(message) => Some(Outcome::Panic(message.clone())),
        }
    }
}

/// `take` が起こし直す失敗。
pub(crate) enum Raise {
    Io { kind: i64, message: String },
    Panic(String),
}

/// `take` の種類。`Run` は根のスコープの最初の失敗を本体の値より先にし、根のスコープを放す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Take {
    Task,
    Run,
}

/// 核の操作の誤り。どれも run を止める。
#[derive(Debug)]
pub(crate) enum CellFault {
    Heap(HeapError),
    Internal(&'static str),
}

impl From<HeapError> for CellFault {
    fn from(error: HeapError) -> CellFault {
        CellFault::Heap(error)
    }
}

impl From<Fault> for CellFault {
    fn from(fault: Fault) -> CellFault {
        match fault {
            Fault::Heap(error) => CellFault::Heap(error),
            _ => CellFault::Internal("a heap operation of the task cells raised a non-heap fault"),
        }
    }
}

enum Phase {
    /// 始める仕事を実行系に渡したが、まだ動いていない。
    Placed,
    Running,
    /// 継続だけになって待つか、実行待ちの列の後ろへ回った。再開する仕事が `Running` に戻す。
    Parked,
    Finished,
}

/// 待ちの登録。
enum Wait {
    Cell(CellRef),
    Ready,
    /// 時間の輪の鍵 `TimerKey` の項。
    Time(TimerKey),
}

/// 時間の輪の鍵。期限はミリ秒で、同じ期限は `seq` で登録の順に並ぶ (docs/spec/vm.md の「時間の待ち」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TimerKey {
    deadline: u64,
    seq: u64,
}

struct TaskCell {
    phase: Phase,
    outcome: Option<Outcome>,
    /// 加わったスコープ。作るときに決まり、`members` の一覧と失敗の記録の先になる。
    scope: CellRef,
    /// タスクの論理的な worker ID。速いモードでは 0 である。
    worker: u32,
    /// 始めたタスクの論理的な ID。始めるときの `transfer_back` に使う。
    spawner: u32,
    /// スコープの属するタスクの一覧の中の位置。
    member_at: u32,
    waiters: Vec<Waiter>,
    /// 待っている間の継続。セルが参照を1つ持つ。
    k: Option<Word>,
    /// タスクが `scope` で開いたいちばん内側のスコープ。開いていなければ `scope` である。
    current: CellRef,
    wait: Option<Wait>,
    cancel: bool,
    /// `shield` の深さ。std にはタスクごとの動的な値を持つ仕組みがなく (タスクは閉じた row で始まり、`var` を
    /// 捕まえられない)、VM の `Task` は起きるたびに作り直すので、タスクのセルに持つ。
    shield: u32,
    /// 速い道の残りの予算。始めるときと再開するときに戻す。
    budget: u32,
    /// 入れ子の `run` で待っている根。
    run: Option<CellRef>,
    detached: bool,
}

struct ScopeCell {
    parent: Option<CellRef>,
    opener: Option<CellRef>,
    flag: bool,
    /// 閉じる印。根は開くときから立つ。
    closing: bool,
    /// 親の入れ子の一覧の中の位置。
    nested_at: u32,
    /// まだ終わっていないタスク。
    members: Vec<CellRef>,
    nested: Vec<CellRef>,
    /// 終わった順で最初の失敗。
    first_failure: Option<Outcome>,
    waiters: Vec<Waiter>,
    closed: bool,
    /// 開くときの開き手の `shield` の深さ。開き手のない根は 0 である。
    shield: u32,
    /// `shield` の境目の印。開いたときの `shield` の深さが、同じタスクが開いた親の深さより大きい (親がほかのタスクのものなら 0 より大きい) スコープに立つ。外の取り消しはここで止まる
    /// (docs/spec/stdlib.md の「取り消し」)。
    boundary: bool,
    /// 加わるタスクを取り消す印。`cancel_members` が立て、属するタスクがなくなったときに `finish` が下ろす
    /// (docs/spec/vm.md の「取り消しの届け方」)。
    cancel_new: bool,
}

/// 印を付ける作業の項。
enum Mark {
    Task(CellRef),
    Scope(CellRef),
}

enum CellKind {
    Task(TaskCell),
    Scope(ScopeCell),
    Chan(ChanCell),
}

/// channel のセル (docs/spec/vm.md の「channel」)。
struct ChanCell {
    cap: u32,
    closed: bool,
    /// 積んだ品物。語は数えた参照で、`depot.publish` で預けたもの。
    items: VecDeque<Word>,
    /// 待っている送り手。項はいつも生きていて、`wait == Some(Wait::Cell(このセル))` である。
    senders: VecDeque<CellRef>,
    /// 待っている受け手。同上。
    receivers: VecDeque<CellRef>,
    /// 品物の預かり先。`new_chan` で開き、セルを放すときに閉じる。
    depot: Depot,
}

impl ChanCell {
    /// 積めるか。待っている受け手1人につき枠が1つ開くので、容量 0 でも受け手が待っていれば積める。
    fn can_send(&self) -> bool {
        self.items.len() < self.cap as usize + self.receivers.len()
    }
}

/// 受け箱の知らせ。落ちたハンドルの知らせはハンドルの `Drop` が、取り消しの知らせは `mark` が積む。
pub(crate) enum Notice {
    HandleDropped(CellRef),
    Cancel(CellRef),
}

/// ほかの worker やハンドルの破棄処理が核へ知らせる受け箱。
#[derive(Default)]
pub(crate) struct Inbox {
    notices: Mutex<Vec<Notice>>,
}

impl Inbox {
    fn push(&self, notice: Notice) {
        self.notices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(notice);
    }

    fn take(&self) -> Vec<Notice> {
        std::mem::take(&mut *self.notices.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// `Task a` の `Foreign` の中身。
pub(crate) struct TaskHandle {
    cell: CellRef,
    inbox: Arc<Inbox>,
    /// `take` が受け取った。受け取ったハンドルはセルをもう指さないので、落ちても知らせない。
    taken: bool,
}

impl Drop for TaskHandle {
    // 破棄処理はヒープの `decref` の中で走るので、セルに触れずに知らせを積むだけにする。前の run のハンドルは、
    // 自分の死んだ受け箱に積むので、生きている登録表には届かない (docs/spec/vm.md の「`Async` のタスク」)
    fn drop(&mut self) {
        if !self.taken {
            self.inbox.push(Notice::HandleDropped(self.cell));
        }
    }
}

/// 登録表が数える仕事の回数。検査モードでなくても数えるが、`RunStats` に写すのは検査モードだけである
/// (docs/spec/vm.md の「`RunStats`」)。
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct TaskStats {
    /// `start` が作ったタスクのセルの数。
    pub(crate) tasks: u64,
    /// `park`、`ready`、`park_time`、`park_drain`、`park_send`、`park_recv` が渡された継続の数。
    pub(crate) parks: u64,
    /// スコープの最初の失敗より後に終わった、失敗のタスクの数。
    pub(crate) dropped_failures: u64,
    /// 取り消しが届いた回数。待つ前に取り消しを見つけた `park`、`park_time`、`park_drain`、`park_send`、`park_recv`、
    /// 受け箱の処理が外した待ち、取り消しで再開する `yield_now`、真を返した `cancel_point`、偽を返した `claim_scope` を
    /// 数える (docs/spec/vm.md の「`RunStats`」)。
    pub(crate) cancels: u64,
    /// 時間の輪に載せた登録の数。`ms > 0` で、地点で取り消されなかった `park_time` だけを数える。
    pub(crate) timers: u64,
    /// channel に品物を積んだ回数と取った回数の和。
    pub(crate) channel_ops: u64,
    /// `park_send` と `park_recv` が渡された継続の数。`parks` にも数える。
    pub(crate) channel_waits: u64,
    /// `sweep_all` が放した channel の数。
    pub(crate) channels_swept: u64,
}

/// 1回の実行のセルの表。実行ごとに作る (docs/spec/vm.md の「`Async` のタスク」)。
pub(crate) struct Registry {
    slots: Vec<(u32, Option<CellKind>)>,
    free: Vec<u32>,
    /// 新しい添字の最初の generation。前の run の `Scope` の値がこの run のセルを指さないようにする。
    floor: u32,
    pub(crate) inbox: Arc<Inbox>,
    pub(crate) stats: TaskStats,
    /// 時間の輪。項は時間を待つタスクのセルの番号だけで、語を持たない (docs/spec/vm.md の「時間の待ち」)。
    timers: BTreeMap<TimerKey, CellRef>,
    /// 次に登録するタイマーの `seq`。
    timer_seq: u64,
    /// `Std.Atomic.Counter` の表。添字は `Machine` が run をまたいで持つので、回復でも戻さない。
    pub(crate) counters: Vec<i64>,
}

impl Registry {
    pub(crate) fn new(floor: u32) -> Registry {
        Registry {
            slots: Vec::new(),
            free: Vec::new(),
            floor,
            inbox: Arc::new(Inbox::default()),
            stats: TaskStats::default(),
            timers: BTreeMap::new(),
            timer_seq: 0,
            counters: Vec::new(),
        }
    }

    /// いちばん早いタイマーの期限。輪が空なら `None` である。
    pub(crate) fn next_deadline(&self) -> Option<u64> {
        self.timers.first_key_value().map(|(key, _)| key.deadline)
    }

    /// 期限が `now_ms` までのタイマーを、期限の順、同じ期限なら登録の順に外し、待っていたタスクを再開する仕事を
    /// `out` に積む。輪の項と待ちの登録は、どちらが先に外れても他方を残さない (docs/spec/vm.md の「時間の待ち」)。
    pub(crate) fn expire(&mut self, now_ms: u64, out: &mut Vec<Runnable>) -> Result<(), CellFault> {
        while let Some(entry) = self.timers.first_entry() {
            let key = *entry.key();
            if key.deadline > now_ms {
                break;
            }
            let cell = entry.remove();
            let task = self.task_mut(cell)?;
            if !matches!(task.phase, Phase::Parked)
                || !matches!(task.wait, Some(Wait::Time(k)) if k == key)
            {
                return Err(CellFault::Internal("a timer names no waiting task"));
            }
            task.wait = None;
            let k = task
                .k
                .take()
                .ok_or(CellFault::Internal("a parked task has no continuation"))?;
            out.push(Runnable::Resume {
                cell,
                k,
                wake: WOKE,
            });
        }
        Ok(())
    }
}

impl Registry {
    fn alloc(&mut self, kind: CellKind) -> CellRef {
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                self.slots.push((self.floor, None));
                u32::try_from(self.slots.len() - 1).expect("the cells fit in u32 positions")
            }
        };
        let slot = &mut self.slots[index as usize];
        slot.1 = Some(kind);
        CellRef {
            index,
            generation: slot.0,
        }
    }

    /// 添字を使い回せるようにし、generation を1つ進めて古い番号を見分ける。
    fn release(&mut self, index: u32) {
        let slot = &mut self.slots[index as usize];
        slot.0 = slot.0.wrapping_add(1);
        slot.1 = None;
        self.free.push(index);
    }

    fn cell_mut(&mut self, cell: CellRef) -> Result<&mut CellKind, CellFault> {
        self.live(cell)
            .ok_or(CellFault::Internal("a cell reference names a freed cell"))
    }

    fn live(&mut self, cell: CellRef) -> Option<&mut CellKind> {
        match self.slots.get_mut(cell.index as usize) {
            Some((generation, Some(kind))) if *generation == cell.generation => Some(kind),
            _ => None,
        }
    }

    fn task_mut(&mut self, cell: CellRef) -> Result<&mut TaskCell, CellFault> {
        match self.cell_mut(cell)? {
            CellKind::Task(task) => Ok(task),
            CellKind::Scope(_) | CellKind::Chan(_) => Err(CellFault::Internal(
                "a task cell reference names another kind of cell",
            )),
        }
    }

    fn scope_mut(&mut self, cell: CellRef) -> Result<&mut ScopeCell, CellFault> {
        match self.cell_mut(cell)? {
            CellKind::Scope(scope) => Ok(scope),
            CellKind::Task(_) | CellKind::Chan(_) => Err(CellFault::Internal(
                "a scope cell reference names another kind of cell",
            )),
        }
    }

    /// タスクの論理的な worker ID。
    pub(crate) fn worker(&mut self, cell: CellRef) -> Result<u32, CellFault> {
        Ok(self.task_mut(cell)?.worker)
    }

    /// 次の run の `floor`。この run が出したどの generation よりも大きい。`slots` は run の間に縮まない。
    /// 表の中の `Counter`。添字が表の外なのは、検査を通ったプログラムからは起きない。
    pub(crate) fn counter(&mut self, i: i64) -> Result<&mut i64, CellFault> {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.counters.get_mut(i))
            .ok_or(CellFault::Internal("a counter index is out of range"))
    }

    pub(crate) fn next_floor(&self) -> u32 {
        self.slots
            .iter()
            .map(|(generation, _)| generation.wrapping_add(1))
            .fold(self.floor, u32::max)
    }

    /// タスクが新しいタスクを加える今のスコープ。
    pub(crate) fn current_of(&mut self, cell: CellRef) -> Result<CellRef, CellFault> {
        Ok(self.task_mut(cell)?.current)
    }

    /// スコープが閉じているか。
    pub(crate) fn closed(&mut self, scope: CellRef) -> Result<bool, CellFault> {
        Ok(self.scope_mut(scope)?.closed)
    }

    /// スコープのセルを開いて親の一覧に積む。旗は呼ぶ側が載せた後に `inherit_flag` で写す。
    fn new_scope(
        &mut self,
        parent: Option<CellRef>,
        opener: Option<CellRef>,
        closing: bool,
    ) -> Result<CellRef, CellFault> {
        let nested_at = match parent {
            Some(p) => self.scope_mut(p)?.nested.len() as u32,
            None => 0,
        };
        let level = match opener {
            Some(o) => self.task_mut(o)?.shield,
            None => 0,
        };
        // 親を同じタスクが開いていれば、その深さより深いときだけ境目になる。ほかの親は `shield` の外にある
        let base = match parent {
            Some(p) if opener.is_some() => {
                let ps = self.scope_mut(p)?;
                if ps.opener == opener { ps.shield } else { 0 }
            }
            _ => 0,
        };
        let s = self.alloc(CellKind::Scope(ScopeCell {
            parent,
            opener,
            flag: false,
            closing,
            nested_at,
            members: Vec::new(),
            nested: Vec::new(),
            first_failure: None,
            waiters: Vec::new(),
            closed: false,
            shield: level,
            boundary: level > base,
            cancel_new: false,
        }));
        if let Some(p) = parent {
            self.scope_mut(p)?.nested.push(s);
        }
        Ok(s)
    }

    /// 親の旗と開いたタスクのビットを新しいスコープに写す。一覧と `current` (根なら `run`) に載せた後に呼ぶ。
    /// 印を付ける側はビットと旗を立ててから一覧と `current` を読むので、どちらが先でも新しいスコープは旗を得る
    /// (docs/spec/vm.md の「取り消しの届け方」)。境目は何も写さない。ビットは `shield` の外の取り消しから来ることが
    /// あるので、写すのは深さ 0 で開くスコープだけである (docs/spec/stdlib.md の「取り消し」)。
    fn inherit_flag(&mut self, s: CellRef) -> Result<(), CellFault> {
        let scope = self.scope_mut(s)?;
        if scope.boundary {
            return Ok(());
        }
        let (parent, opener, level) = (scope.parent, scope.opener, scope.shield);
        let mut flag = match parent {
            Some(p) => self.scope_mut(p)?.flag,
            None => false,
        };
        if let Some(o) = opener
            && level == 0
        {
            flag |= self.task_mut(o)?.cancel;
        }
        self.scope_mut(s)?.flag = flag;
        Ok(())
    }

    /// 根のスコープを開く。入れ子の `run` の根は呼んだタスク `caller` を開いたタスクとして持つ。
    pub(crate) fn open_root(
        &mut self,
        parent: Option<CellRef>,
        caller: Option<CellRef>,
    ) -> Result<CellRef, CellFault> {
        let s = self.new_scope(parent, caller, true)?;
        if let Some(c) = caller {
            self.task_mut(c)?.run = Some(s);
        }
        self.inherit_flag(s)?;
        Ok(s)
    }

    /// スコープ `scope` に新しいタスクを置き、ハンドルと始める仕事を返す。`spawner` は呼んだタスクの論理的な ID で、
    /// `g` は入口に渡す閉包である。新しい ID に渡す前に、`g` とまだ印のないセッションの根 `roots` に共有の印を付ける
    /// (`par` の fork と同じ。docs/spec/vm.md の「`Async` のタスク」)。
    pub(crate) fn start<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        scope: CellRef,
        spawner: u32,
        g: Word,
        roots: &[Word],
    ) -> Result<Option<(Word, Runnable)>, CellFault> {
        let s = self.scope_mut(scope)?;
        if s.closed {
            return Ok(None);
        }
        let (flag, member_at) = (s.flag || s.cancel_new, s.members.len() as u32);
        // 印付けは送り手の ID のまま行い、`Ctx` の持つ語には触れない。値から届く evidence は退いたものだけである
        // (docs/spec/object-model.md の「evidence の3つの物体」)
        let heap = unsafe { ctx.heap_mut() };
        let worker = heap.fork_worker();
        for &root in roots {
            unsafe { heap.share(root, worker) }?;
        }
        unsafe { heap.share(g, worker) }?;
        let cell = self.alloc(CellKind::Task(TaskCell {
            phase: Phase::Placed,
            outcome: None,
            scope,
            worker,
            spawner,
            member_at,
            waiters: Vec::new(),
            k: None,
            current: scope,
            wait: None,
            cancel: flag,
            shield: 0,
            budget: BUDGET,
            run: None,
            detached: false,
        }));
        self.scope_mut(scope)?.members.push(cell);
        self.stats.tasks += 1;
        let handle = heap.alloc_foreign(
            TaskHandle {
                cell,
                inbox: Arc::clone(&self.inbox),
                taken: false,
            },
            true,
        );
        Ok(Some((handle, Runnable::Start { cell, closure: g })))
    }

    /// 始める仕事を実行系が動かし始めた。始めたタスクの ID を返す。実行系は、その ID から閉包を `transfer_back` する。
    pub(crate) fn started(&mut self, cell: CellRef) -> Result<u32, CellFault> {
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Placed) {
            return Err(CellFault::Internal("a task starts twice"));
        }
        task.phase = Phase::Running;
        task.budget = BUDGET;
        Ok(task.spawner)
    }

    /// 再開する仕事を実行系が動かし始めた。渡す `Wake` を返す。実行待ちから再開するタスクが取り消されていれば
    /// `Cancelled` に替える。
    pub(crate) fn resumed(&mut self, cell: CellRef, wake: i64) -> Result<i64, CellFault> {
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Parked) {
            return Err(CellFault::Internal("a task resumes without waiting"));
        }
        task.phase = Phase::Running;
        task.budget = BUDGET;
        let ready = matches!(task.wait.take(), Some(Wait::Ready));
        if ready && wake == WOKE && self.point(cell)? {
            self.stats.cancels += 1;
            return Ok(CANCELLED);
        }
        Ok(wake)
    }

    /// 取り消しの地点の規則。`shield` の中では、いちばん内側の `shield` の中で自分が開いたスコープの旗だけを見る
    /// (docs/spec/vm.md の「取り消しの届け方」)。
    fn point(&mut self, cell: CellRef) -> Result<bool, CellFault> {
        let task = self.task_mut(cell)?;
        let (depth, bit, current) = (task.shield, task.cancel, task.current);
        if depth == 0 {
            return Ok(bit || self.scope_mut(current)?.flag);
        }
        let scope = self.scope_mut(current)?;
        Ok(scope.flag && scope.opener == Some(cell) && scope.shield == depth)
    }

    /// 借りたハンドル `handle` のセルの番号を `Std.Async.Cell` の即値で返す。
    pub(crate) fn cell_of<M: Mode>(&self, ctx: &Ctx<M>, handle: Word) -> Result<i64, CellFault> {
        Ok(self.handle_cell(ctx, handle)?.encode())
    }

    /// タスク `me` が、継続 `k` でセル `c` が終わるか閉じるのを待つ。もう終わっていれば、すぐ再開する仕事を返す。
    /// どちらでも、`k` は継続の待ちか返す仕事に移る。終わったセルにも近道をせず、いつも継続を渡して再開する。
    pub(crate) fn park(
        &mut self,
        c: i64,
        me: CellRef,
        k: Word,
    ) -> Result<Option<Runnable>, CellFault> {
        let target = CellRef::decode(c);
        let done = match self.cell_mut(target)? {
            CellKind::Task(task) => matches!(task.phase, Phase::Finished),
            CellKind::Scope(scope) => scope.closed,
            CellKind::Chan(_) => return Err(CellFault::Internal("a cell of the wrong kind")),
        };
        self.suspend(me)?;
        self.stats.parks += 1;
        if self.point(me)? {
            self.stats.cancels += 1;
            return Ok(Some(Runnable::Resume {
                cell: me,
                k,
                wake: CANCELLED,
            }));
        }
        if done {
            return Ok(Some(Runnable::Resume {
                cell: me,
                k,
                wake: WOKE,
            }));
        }
        let task = self.task_mut(me)?;
        task.k = Some(k);
        task.wait = Some(Wait::Cell(target));
        let waiters = match self.cell_mut(target)? {
            CellKind::Task(task) => &mut task.waiters,
            CellKind::Scope(scope) => &mut scope.waiters,
            CellKind::Chan(_) => return Err(CellFault::Internal("a cell of the wrong kind")),
        };
        waiters.push(Waiter::Task(me));
        Ok(None)
    }

    /// タスク `me` が、継続 `k` でスコープ `s` に属するタスクがなくなるまで待つ。`s` は閉じない。閉じたスコープ、
    /// 離れたスコープ、前の run のスコープ、空のスコープでは、すぐ再開する仕事を返す (docs/spec/vm.md の「命令」)。
    pub(crate) fn park_drain(
        &mut self,
        s: i64,
        me: CellRef,
        k: Word,
    ) -> Result<Option<Runnable>, CellFault> {
        let target = match self.scope_cell(s) {
            Some(t) => {
                let scope = self.scope_mut(t)?;
                (!scope.closed && !scope.members.is_empty()).then_some(t)
            }
            None => None,
        };
        self.suspend(me)?;
        self.stats.parks += 1;
        if self.point(me)? {
            self.stats.cancels += 1;
            return Ok(Some(Runnable::Resume {
                cell: me,
                k,
                wake: CANCELLED,
            }));
        }
        let Some(target) = target else {
            return Ok(Some(Runnable::Resume {
                cell: me,
                k,
                wake: WOKE,
            }));
        };
        let task = self.task_mut(me)?;
        task.k = Some(k);
        task.wait = Some(Wait::Cell(target));
        // 閉じないスコープは何度も待たれるので、取り消しで外れた古い項をここで除き、待ち手が増え続けないようにする
        let old = std::mem::take(&mut self.scope_mut(target)?.waiters);
        let mut kept = Vec::with_capacity(old.len() + 1);
        for waiter in old {
            let keep = match waiter {
                Waiter::Task(t) => {
                    t != me
                        && matches!(self.live(t), Some(CellKind::Task(task))
                            if matches!(task.wait, Some(Wait::Cell(c)) if c == target))
                }
                Waiter::Token(_) => true,
            };
            if keep {
                kept.push(waiter);
            }
        }
        kept.push(Waiter::Task(me));
        self.scope_mut(target)?.waiters = kept;
        Ok(None)
    }

    /// タスク `me` が、継続 `k` で実行待ちの列の後ろへ回る。
    pub(crate) fn ready(&mut self, me: CellRef, k: Word) -> Result<Runnable, CellFault> {
        self.suspend(me)?;
        self.stats.parks += 1;
        self.task_mut(me)?.wait = Some(Wait::Ready);
        Ok(Runnable::Resume {
            cell: me,
            k,
            wake: WOKE,
        })
    }

    /// タスク `me` が、継続 `k` で `ms` ミリ秒眠る。`now_ms` は切り上げて読んだ今の時刻である。0 以下は譲りにして
    /// 輪に載せないので、VM は時計を読まず、次に集める地点までの遅れもない (docs/spec/vm.md の「時間の待ち」)。
    pub(crate) fn park_time(
        &mut self,
        now_ms: u64,
        ms: i64,
        me: CellRef,
        k: Word,
    ) -> Result<Option<Runnable>, CellFault> {
        self.suspend(me)?;
        self.stats.parks += 1;
        if self.point(me)? {
            self.stats.cancels += 1;
            return Ok(Some(Runnable::Resume {
                cell: me,
                k,
                wake: CANCELLED,
            }));
        }
        if ms <= 0 {
            self.task_mut(me)?.wait = Some(Wait::Ready);
            return Ok(Some(Runnable::Resume {
                cell: me,
                k,
                wake: WOKE,
            }));
        }
        // 期限は飽和させ、どんな長さも誤りにしない。仮想の時計が端に着いた後は、どの期限も今と等しくなる
        let key = TimerKey {
            deadline: now_ms.saturating_add(ms as u64),
            seq: self.timer_seq,
        };
        self.timer_seq += 1;
        self.timers.insert(key, me);
        self.stats.timers += 1;
        let task = self.task_mut(me)?;
        task.k = Some(k);
        task.wait = Some(Wait::Time(key));
        Ok(None)
    }

    fn suspend(&mut self, me: CellRef) -> Result<(), CellFault> {
        let task = self.task_mut(me)?;
        if !matches!(task.phase, Phase::Running) {
            return Err(CellFault::Internal("a task waits without running"));
        }
        task.phase = Phase::Parked;
        Ok(())
    }

    /// ハンドルのセル。ハンドルの受け箱が登録表の受け箱と違えば、前の run のハンドルである。ハンドルは落ちたときに
    /// 知らせを積むために自分の run の受け箱を持つので、それを比べれば generation の一周によらずに run を見分けられる。
    /// `Task a` は `Lin` で入力をまたいで残らないので、検査を通ったプログラムからは届かない
    /// (docs/spec/vm.md の「`Async` のタスク」)。
    fn handle_cell<M: Mode>(&self, ctx: &Ctx<M>, handle: Word) -> Result<CellRef, CellFault> {
        // ハンドルは `Task a` の `Foreign` で、呼ぶ側が持つ唯一の参照である (`Task a` は `Lin`)
        let h = unsafe { ctx.heap().foreign_unique::<TaskHandle>(handle) }?;
        let h = unsafe { &*h };
        if !Arc::ptr_eq(&h.inbox, &self.inbox) {
            return Err(CellFault::Internal("a task handle outlived its run"));
        }
        Ok(h.cell)
    }

    /// `cell` が終わるか閉じるまで、token で待つ。
    pub(crate) fn wait(&mut self, cell: CellRef, token: WaitToken) -> Result<(), CellFault> {
        let waiters = match self.cell_mut(cell)? {
            CellKind::Task(task) => &mut task.waiters,
            CellKind::Scope(scope) => &mut scope.waiters,
            CellKind::Chan(_) => return Err(CellFault::Internal("a cell of the wrong kind")),
        };
        waiters.push(Waiter::Token(token));
        Ok(())
    }

    /// タスク `cell` を `outcome` で終える。起こす仕事を `out` に積む。失敗はスコープの最初の失敗として記録する。
    /// スコープの最後のタスクなら、閉じる印のあるスコープを閉じ、加わるタスクを取り消す印を下ろし、スコープの待ち手を
    /// すべて起こす。`park_drain` の待ち手は閉じる印のないスコープも待つ。ハンドルがもう落ちていれば、セルをすぐ放す。
    /// `depth_offset` は呼んだ実行系のタスクの深さの起点で、セルを放すときに `Ctx` へ戻す。
    pub(crate) fn finish<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        cell: CellRef,
        outcome: Outcome,
        depth_offset: usize,
        out: &mut Vec<Runnable>,
    ) -> Result<(), CellFault> {
        let failure = outcome.failure();
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Running) {
            return Err(CellFault::Internal("a task finishes without running"));
        }
        let (scope, at, detached) = (task.scope, task.member_at, task.detached);
        self.remove_member(scope, at)?;
        let task = self.task_mut(cell)?;
        task.phase = Phase::Finished;
        task.outcome = Some(outcome);
        for waiter in std::mem::take(&mut task.waiters) {
            self.wake_waiter(waiter, cell, out)?;
        }
        if let Some(failure) = failure {
            let s = self.scope_mut(scope)?;
            if s.first_failure.is_none() {
                s.first_failure = Some(failure);
            } else {
                self.stats.dropped_failures += 1;
            }
            self.mark(Mark::Scope(scope))?;
        }
        let s = self.scope_mut(scope)?;
        if s.members.is_empty() {
            s.closed |= s.closing;
            s.cancel_new = false;
            for waiter in std::mem::take(&mut s.waiters) {
                self.wake_waiter(waiter, scope, out)?;
            }
        }
        if detached {
            self.free_task(ctx, cell, depth_offset)?;
        }
        Ok(())
    }

    /// 起こす待ち手。継続で待つタスクは、セルの継続を取り出して再開する。
    /// `from` が終わるか閉じたときの起こし。取り消しで外した待ちの項は待ち手に残るので、タスクが今も `from` を
    /// 待っているときだけ起こす。
    fn wake_waiter(
        &mut self,
        waiter: Waiter,
        from: CellRef,
        out: &mut Vec<Runnable>,
    ) -> Result<(), CellFault> {
        match waiter {
            Waiter::Task(t) => {
                if let Some(CellKind::Task(task)) = self.live(t)
                    && matches!(task.wait, Some(Wait::Cell(c)) if c == from)
                {
                    task.wait = None;
                    let k = task
                        .k
                        .take()
                        .ok_or(CellFault::Internal("a parked task has no continuation"))?;
                    out.push(Runnable::Resume {
                        cell: t,
                        k,
                        wake: WOKE,
                    });
                }
            }
            Waiter::Token(token) => out.push(Runnable::Token(token)),
        }
        Ok(())
    }

    fn remove_member(&mut self, scope: CellRef, at: u32) -> Result<(), CellFault> {
        let s = self.scope_mut(scope)?;
        s.members.swap_remove(at as usize);
        if let Some(&moved) = s.members.get(at as usize) {
            self.task_mut(moved)?.member_at = at;
        }
        Ok(())
    }

    /// 閉じたスコープのセルを、親の一覧から外してから放す。
    fn release_scope(&mut self, s: CellRef) -> Result<(), CellFault> {
        let scope = self.scope_mut(s)?;
        let (parent, at, opener) = (scope.parent, scope.nested_at, scope.opener);
        if let Some(o) = opener {
            let task = self.task_mut(o)?;
            if task.run == Some(s) {
                task.run = None;
            }
        }
        if let Some(p) = parent {
            let p = self.scope_mut(p)?;
            p.nested.swap_remove(at as usize);
            if let Some(&moved) = p.nested.get(at as usize) {
                self.scope_mut(moved)?.nested_at = at;
            }
        }
        self.release(s.index);
        Ok(())
    }

    /// ハンドル `handle` を消費して、終わったタスクの値を返すか、失敗を返す。`handle` は呼ぶ側が持つ参照である。
    /// 値は終わったタスクの ID から今の worker へ戻し、セルを放す。
    pub(crate) fn take<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        handle: Word,
        kind: Take,
        depth_offset: usize,
    ) -> Result<Result<Word, Raise>, CellFault> {
        let cell = self.handle_cell(ctx, handle)?;
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Finished) {
            return Err(CellFault::Internal("awaiting an unfinished task"));
        }
        let (worker, scope) = (task.worker, task.scope);
        let outcome = task
            .outcome
            .take()
            .ok_or(CellFault::Internal("a finished task has no outcome"))?;
        let mut raised = None;
        if kind == Take::Run {
            let root = self.scope_mut(scope)?;
            if !root.closed {
                return Err(CellFault::Internal("a run ends before its scope closes"));
            }
            raised = root.first_failure.take();
            self.release_scope(scope)?;
        }
        let result = match (raised, outcome) {
            (Some(failure), outcome) => {
                if let Outcome::Value(w) = outcome {
                    Depot::of(worker).discard(ctx, w, depth_offset)?;
                }
                Err(raise(failure))
            }
            (None, Outcome::Value(w)) => {
                // 値はタスクが `finish_ok` に預けた参照で、受け取る側だけが持つ
                Ok(Depot::of(worker).receive(ctx, w)?)
            }
            (None, failure) => Err(raise(failure)),
        };
        // ハンドルの参照は呼ぶ側から移ってきたもので、ここで手放す。受け取った印を先に付けるので、破棄処理は
        // 知らせを積まない
        self.give_up(ctx, handle)?;
        self.free_task(ctx, cell, depth_offset)?;
        Ok(result)
    }

    /// ハンドルに受け取った印を付けて手放す。印があるので破棄処理は知らせを積まない。
    fn give_up<M: Mode>(&mut self, ctx: &mut Ctx<M>, handle: Word) -> Result<(), CellFault> {
        unsafe {
            (*ctx.heap().foreign_unique::<TaskHandle>(handle)?).taken = true;
            ctx.heap_mut().decref(handle)?;
        }
        Ok(())
    }

    /// `detach` と `cancel_raw`。ハンドルを手放し、終わったタスクのセルを放すか、持ち手のないタスクにする。
    pub(crate) fn detach<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        handle: Word,
        cancel: bool,
        depth_offset: usize,
    ) -> Result<(), CellFault> {
        let cell = self.handle_cell(ctx, handle)?;
        self.give_up(ctx, handle)?;
        self.let_go(ctx, cell, cancel, depth_offset)
    }

    fn let_go<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        cell: CellRef,
        cancel: bool,
        depth_offset: usize,
    ) -> Result<(), CellFault> {
        let task = self.task_mut(cell)?;
        if matches!(task.phase, Phase::Finished) {
            return self.free_task(ctx, cell, depth_offset);
        }
        task.detached = true;
        if cancel {
            self.mark(Mark::Task(cell))?;
        }
        Ok(())
    }

    pub(crate) fn shield_enter(&mut self, me: CellRef) -> Result<(), CellFault> {
        self.task_mut(me)?.shield += 1;
        Ok(())
    }

    pub(crate) fn shield_leave(&mut self, me: CellRef) -> Result<(), CellFault> {
        let task = self.task_mut(me)?;
        task.shield = task
            .shield
            .checked_sub(1)
            .ok_or(CellFault::Internal("a shield is left without entering"))?;
        Ok(())
    }

    /// 取り消しの地点が投げるかを答える。タスクの外では偽である。真の答えは取り消しが届いたものとして数える。
    pub(crate) fn cancel_point(&mut self, me: Option<CellRef>) -> Result<bool, CellFault> {
        let Some(me) = me else {
            return Ok(false);
        };
        let point = self.point(me)?;
        self.stats.cancels += u64::from(point);
        Ok(point)
    }

    /// 速い道の予算を1つ使う。使い切ったら真を返し、再開まで真のままである。
    pub(crate) fn spend_budget(&mut self, me: CellRef) -> Result<bool, CellFault> {
        let task = self.task_mut(me)?;
        task.budget = task.budget.saturating_sub(1);
        Ok(task.budget == 0)
    }

    /// 生きているスコープのセル。閉じていても返す。離れたスコープと前の run のスコープは `None` である。
    pub(crate) fn scope_cell(&mut self, s: i64) -> Option<CellRef> {
        let cell = CellRef::decode(s);
        match self.live(cell) {
            Some(CellKind::Scope(_)) => Some(cell),
            _ => None,
        }
    }

    /// `scope` の本体のスコープを開いて、`me` の今のスコープにする。
    pub(crate) fn open_scope(&mut self, me: CellRef) -> Result<i64, CellFault> {
        let parent = self.task_mut(me)?.current;
        let s = self.new_scope(Some(parent), Some(me), false)?;
        self.task_mut(me)?.current = s;
        self.inherit_flag(s)?;
        Ok(s.encode())
    }

    /// 閉じる印を付け、待つセルの番号を返す。タスクが残っていなければすぐ閉じる。
    pub(crate) fn close_scope(&mut self, s: i64) -> Result<i64, CellFault> {
        let cell = self
            .scope_cell(s)
            .ok_or(CellFault::Internal("a scope closes after it is left"))?;
        let scope = self.scope_mut(cell)?;
        scope.closing = true;
        if scope.members.is_empty() {
            scope.closed = true;
        }
        Ok(s)
    }

    /// 閉じたスコープを離れ、今のスコープを親に戻す。最初の失敗を返す。
    pub(crate) fn leave_scope(&mut self, s: i64, me: CellRef) -> Result<Option<Raise>, CellFault> {
        let cell = self
            .scope_cell(s)
            .ok_or(CellFault::Internal("a scope is left twice"))?;
        let scope = self.scope_mut(cell)?;
        if !scope.closed {
            return Err(CellFault::Internal("a scope is left before it closes"));
        }
        let (parent, failure) = (scope.parent, scope.first_failure.take());
        let task = self.task_mut(me)?;
        if task.current != cell {
            return Err(CellFault::Internal("a task leaves a scope it is not in"));
        }
        task.current = parent.ok_or(CellFault::Internal("a named scope has no parent"))?;
        self.release_scope(cell)?;
        Ok(failure.map(raise))
    }

    /// `cancel_scope`。閉じたスコープには何もしない。
    pub(crate) fn cancel_scope(&mut self, s: i64) -> Result<(), CellFault> {
        if let Some(cell) = self.scope_cell(s)
            && !self.scope_mut(cell)?.closed
        {
            self.mark(Mark::Scope(cell))?;
        }
        Ok(())
    }

    /// `race` の勝ちを決める。呼んだタスクのビットか今のスコープの旗が立っていれば、取り消しが届いたものとして数えて偽を
    /// 返す。そうでなければ、同じ呼び出しの中で `s` に旗を立てて真を返す。`shield` は見ない
    /// (docs/spec/vm.md の「命令」)。
    pub(crate) fn claim_scope(&mut self, s: i64, me: CellRef) -> Result<bool, CellFault> {
        let task = self.task_mut(me)?;
        let (bit, current) = (task.cancel, task.current);
        if bit || self.scope_mut(current)?.flag {
            self.stats.cancels += 1;
            return Ok(false);
        }
        self.cancel_scope(s)?;
        Ok(true)
    }

    /// `s` に属するタスクをそれぞれ取り消し、`s` が空になるまで加わるタスクも取り消す。`s` の旗は立てないので、`s` の
    /// 本体は取り消さない。印は一覧を読む前に立て、間に加わるタスクを取りこぼさない
    /// (docs/spec/vm.md の「取り消しの届け方」)。
    pub(crate) fn cancel_members(&mut self, s: i64) -> Result<(), CellFault> {
        if let Some(cell) = self.scope_cell(s)
            && !self.scope_mut(cell)?.closed
        {
            let scope = self.scope_mut(cell)?;
            scope.cancel_new = !scope.members.is_empty();
            let members = scope.members.clone();
            for m in members {
                self.mark(Mark::Task(m))?;
            }
        }
        Ok(())
    }

    /// `scope` の本体の失敗を記録し、子をすべて取り消す。種類 `Cancelled` は記録しない。
    pub(crate) fn fail_scope(&mut self, s: i64, failure: Outcome) -> Result<(), CellFault> {
        let cell = self
            .scope_cell(s)
            .ok_or(CellFault::Internal("a scope fails after it is left"))?;
        if let Some(failure) = failure.failure() {
            let scope = self.scope_mut(cell)?;
            if scope.first_failure.is_none() {
                scope.first_failure = Some(failure);
            } else {
                self.stats.dropped_failures += 1;
            }
        }
        self.mark(Mark::Scope(cell))
    }

    /// 取り消しの印を付ける。ビットと旗を立てて知らせを積むだけで、待ちは外さない。作業の列でたどる。
    fn mark(&mut self, first: Mark) -> Result<(), CellFault> {
        let mut work = vec![first];
        while let Some(m) = work.pop() {
            match m {
                Mark::Task(t) => {
                    let task = self.task_mut(t)?;
                    if task.cancel {
                        continue;
                    }
                    task.cancel = true;
                    // 開いているスコープは、入れ子の `run` の根から今のスコープを経て親へ、このタスクが開いたスコープが
                    // 続く間である。内から外へ並べ、いちばん外の境目とその内側には印を付けない
                    // (docs/spec/vm.md の「取り消しの届け方」)
                    let (mut s, run, home) = (task.current, task.run, task.scope);
                    let mut chain = Vec::new();
                    if let Some(r) = run {
                        chain.push((r, self.scope_mut(r)?.boundary));
                    }
                    while s != home {
                        let scope = self.scope_mut(s)?;
                        chain.push((s, scope.boundary));
                        s = scope
                            .parent
                            .ok_or(CellFault::Internal("a named scope has no parent"))?;
                    }
                    let cut = chain.iter().rposition(|&(_, b)| b).map_or(0, |i| i + 1);
                    work.extend(chain[cut..].iter().map(|&(c, _)| Mark::Scope(c)));
                    self.inbox.push(Notice::Cancel(t));
                }
                Mark::Scope(s) => {
                    let scope = self.scope_mut(s)?;
                    if scope.flag {
                        continue;
                    }
                    scope.flag = true;
                    work.extend(scope.members.iter().map(|&m| Mark::Task(m)));
                    // 入れ子の境目は外の取り消しを止める
                    let nested = scope.nested.clone();
                    for n in nested {
                        if !self.scope_mut(n)?.boundary {
                            work.push(Mark::Scope(n));
                        }
                    }
                    let scope = self.scope_mut(s)?;
                    // 本体のタスクにはビットを立てず、届けるだけにする。外すかは `unpark` が持ち主の側で規則を確かめて決める
                    if let Some(o) = scope.opener {
                        self.inbox.push(Notice::Cancel(o));
                    }
                }
            }
        }
        Ok(())
    }

    /// 取り消しの知らせを受けたタスクの待ちを外す。タスクとスコープの待ち手の項は残し、起こす側が `wait` で見分ける。
    /// channel の待ち手の列の項はいつも生きているので、ここで外す。
    fn unpark(&mut self, cell: CellRef, out: &mut Vec<Runnable>) -> Result<(), CellFault> {
        let Some(CellKind::Task(task)) = self.live(cell) else {
            return Ok(());
        };
        if !matches!(task.phase, Phase::Parked)
            || !matches!(task.wait, Some(Wait::Cell(_) | Wait::Time(_)))
        {
            return Ok(());
        }
        if !self.point(cell)? {
            return Ok(());
        }
        let task = self.task_mut(cell)?;
        match task.wait.take() {
            // 輪の項を残すと、満期に `expire` が待たないタスクを名指して誤りにする
            Some(Wait::Time(key)) => {
                self.timers.remove(&key);
            }
            Some(Wait::Cell(c)) => {
                if let Some(chan) = self.chan_mut(c) {
                    chan.senders.retain(|&t| t != cell);
                    chan.receivers.retain(|&t| t != cell);
                }
            }
            Some(Wait::Ready) | None => {}
        }
        let task = self.task_mut(cell)?;
        let k = task
            .k
            .take()
            .ok_or(CellFault::Internal("a parked task has no continuation"))?;
        self.stats.cancels += 1;
        out.push(Runnable::Resume {
            cell,
            k,
            wake: CANCELLED,
        });
        Ok(())
    }

    /// 受け箱の知らせを処理し、取り消しで待ちを外したタスクの再開を `out` に積む。処理が新しい知らせを積むので、
    /// 受け箱が空になるまで繰り返す。
    pub(crate) fn drain<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        depth_offset: usize,
        out: &mut Vec<Runnable>,
    ) -> Result<(), CellFault> {
        loop {
            let notices = self.inbox.take();
            if notices.is_empty() {
                return Ok(());
            }
            for notice in notices {
                match notice {
                    Notice::HandleDropped(cell) => self.let_go(ctx, cell, true, depth_offset)?,
                    Notice::Cancel(cell) => self.unpark(cell, out)?,
                }
            }
        }
    }

    /// run の終わりに、残ったセルをすべて放す。閉じていない channel と品物の残った channel もここで放す。その間に
    /// 積まれた知らせは受け箱ごと捨てる。
    pub(crate) fn sweep_all<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        depth_offset: usize,
    ) -> Result<(), CellFault> {
        for index in 0..self.slots.len() {
            let (generation, kind) = &self.slots[index];
            let cell = CellRef {
                index: index as u32,
                generation: *generation,
            };
            match kind {
                Some(CellKind::Task(_)) => self.free_task(ctx, cell, depth_offset)?,
                Some(CellKind::Scope(_)) => self.release(cell.index),
                Some(CellKind::Chan(_)) => self.free_chan(ctx, cell, depth_offset)?,
                None => {}
            }
        }
        drop(self.inbox.take());
        // 眠るタスクを残して終わる run がある (`exit` や誤り) ので、空であることは確かめない
        self.timers.clear();
        Ok(())
    }

    /// タスクのセルを放す。受け取られなかった値をタスクの ID の下で落とし、ID を退かせる。退かせるときに ID の物体が
    /// 残っていれば、`par` の join と同じく `TransferLeak` がヒープの誤りとして出る。
    fn free_task<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        cell: CellRef,
        depth_offset: usize,
    ) -> Result<(), CellFault> {
        let task = self.task_mut(cell)?;
        let (worker, outcome) = (task.worker, task.outcome.take());
        let mut depot = Depot::of(worker);
        if let Some(Outcome::Value(w)) = outcome {
            depot.discard(ctx, w, depth_offset)?;
        }
        depot.close(ctx)?;
        self.release(cell.index);
        Ok(())
    }
}

/// channel の操作 (docs/spec/vm.md の「channel」)。channel の引数は `Std.Chan.Chan` の即値で、放したセルと前の run の
/// 値は閉じて空の channel として振る舞う。
impl Registry {
    /// 生きた channel のセル。放したセル、前の run の値、タスクかスコープのセルは `None` である。
    fn chan_mut(&mut self, c: CellRef) -> Option<&mut ChanCell> {
        match self.live(c) {
            Some(CellKind::Chan(chan)) => Some(chan),
            _ => None,
        }
    }

    /// 容量 `cap` の channel を作る。容量は `0..=u32::MAX` に収める。
    pub(crate) fn new_chan<M: Mode>(&mut self, ctx: &mut Ctx<M>, cap: i64) -> i64 {
        let cap = cap.clamp(0, i64::from(u32::MAX)) as u32;
        self.alloc(CellKind::Chan(ChanCell {
            cap,
            closed: false,
            items: VecDeque::new(),
            senders: VecDeque::new(),
            receivers: VecDeque::new(),
            depot: Depot::open(ctx),
        }))
        .encode()
    }

    /// タスクの中なら予算を1つ使い、`yields` が真で使い切ったら真を返す。タスクの外では予算を使わない。
    fn spend(&mut self, me: Option<CellRef>, yields: bool) -> Result<bool, CellFault> {
        match me {
            Some(me) => Ok(self.spend_budget(me)? && yields),
            None => Ok(false),
        }
    }

    /// 送りの速い道。0 なら `push_raw` で積んでよく、1 なら待ち、2 なら閉じていて、3 なら先に譲る。
    pub(crate) fn reserve_send(
        &mut self,
        c: i64,
        me: Option<CellRef>,
        yields: bool,
    ) -> Result<i64, CellFault> {
        if self.spend(me, yields)? {
            return Ok(3);
        }
        Ok(match self.chan_mut(CellRef::decode(c)) {
            None => 2,
            Some(chan) if chan.closed => 2,
            Some(chan) if chan.can_send() => 0,
            Some(_) => 1,
        })
    }

    /// 受け取りの速い道。0 なら `pop_raw` で取ってよく、1 なら待ち、2 なら閉じて空で、3 なら先に譲る。
    pub(crate) fn reserve_recv(
        &mut self,
        c: i64,
        me: Option<CellRef>,
        yields: bool,
    ) -> Result<i64, CellFault> {
        if self.spend(me, yields)? {
            return Ok(3);
        }
        Ok(match self.chan_mut(CellRef::decode(c)) {
            None => 2,
            Some(chan) if !chan.items.is_empty() => 0,
            Some(chan) if chan.closed => 2,
            Some(_) => 1,
        })
    }

    /// 0 を返した `reserve_send` の直後に、品物 `x` を預けて積む。待っている受け手がいれば先頭を1つ起こす。`x` は呼ぶ側
    /// から移ってきた参照である。
    pub(crate) fn push_raw<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        x: Word,
        c: i64,
    ) -> Result<Option<Runnable>, CellFault> {
        let cell = CellRef::decode(c);
        let chan = self
            .chan_mut(cell)
            .ok_or(CellFault::Internal("an item is pushed to a freed channel"))?;
        let x = chan.depot.publish(ctx, x)?;
        chan.items.push_back(x);
        let receiver = chan.receivers.pop_front();
        self.stats.channel_ops += 1;
        receiver.map(|r| self.wake(r, cell)).transpose()
    }

    /// 0 を返した `reserve_recv` の直後に、先頭の品物を取って受け取る。積めるようになれば待っている送り手の先頭を1つ
    /// 起こし、閉じて空になれば channel を放す。
    pub(crate) fn pop_raw<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        c: i64,
    ) -> Result<(Word, Option<Runnable>), CellFault> {
        let cell = CellRef::decode(c);
        let chan = self.chan_mut(cell).ok_or(CellFault::Internal(
            "an item is popped from a freed channel",
        ))?;
        let x = chan.items.pop_front().ok_or(CellFault::Internal(
            "an item is popped from an empty channel",
        ))?;
        let x = chan.depot.receive(ctx, x)?;
        let sender = if chan.can_send() {
            chan.senders.pop_front()
        } else {
            None
        };
        self.stats.channel_ops += 1;
        let run = sender.map(|s| self.wake(s, cell)).transpose()?;
        self.settle(ctx, cell)?;
        Ok((x, run))
    }

    /// 閉じる。待っている受け手と送り手をすべて起こし、空なら放す。放したか閉じた channel には何もしない。同じ
    /// channel で受け手と送り手が同時に待つことはないので、起こす順は列ごとの FIFO だけで決まる。
    pub(crate) fn close_raw<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        c: i64,
    ) -> Result<Vec<Runnable>, CellFault> {
        let cell = CellRef::decode(c);
        let Some(chan) = self.chan_mut(cell) else {
            return Ok(Vec::new());
        };
        if chan.closed {
            return Ok(Vec::new());
        }
        chan.closed = true;
        let waiters: Vec<CellRef> = chan
            .receivers
            .drain(..)
            .chain(chan.senders.drain(..))
            .collect();
        let woken = waiters
            .into_iter()
            .map(|t| self.wake(t, cell))
            .collect::<Result<Vec<_>, _>>()?;
        self.settle(ctx, cell)?;
        Ok(woken)
    }

    /// channel の待ち手 `t` を起こす。列から外した後に呼ぶ。枠も品物も取っておかず、起こされたタスクが速い道を
    /// やり直す。
    fn wake(&mut self, t: CellRef, chan: CellRef) -> Result<Runnable, CellFault> {
        let task = self.task_mut(t)?;
        if !matches!(task.phase, Phase::Parked)
            || !matches!(task.wait, Some(Wait::Cell(c)) if c == chan)
        {
            return Err(CellFault::Internal("a channel waiter is not waiting"));
        }
        task.wait = None;
        let k = task
            .k
            .take()
            .ok_or(CellFault::Internal("a parked task has no continuation"))?;
        Ok(Runnable::Resume {
            cell: t,
            k,
            wake: WOKE,
        })
    }

    /// タスク `me` が、継続 `k` で channel `c` に枠が空くか閉じるまで待つ。
    pub(crate) fn park_send(
        &mut self,
        c: i64,
        me: CellRef,
        k: Word,
    ) -> Result<Option<Runnable>, CellFault> {
        self.park_chan(c, me, k, Side::Send)
    }

    /// タスク `me` が、継続 `k` で channel `c` に品物が来るか閉じるまで待つ。
    pub(crate) fn park_recv(
        &mut self,
        c: i64,
        me: CellRef,
        k: Word,
    ) -> Result<Option<Runnable>, CellFault> {
        self.park_chan(c, me, k, Side::Recv)
    }

    /// `park` と同じく、止めて数え、取り消しの規則を確かめる。すぐ戻れるなら `Woke` で戻り、そうでなければ待ち手の列に
    /// 登録する。受け手の登録は枠を1つ開くので、積めるようになれば送り手の先頭を1つ起こす。
    fn park_chan(
        &mut self,
        c: i64,
        me: CellRef,
        k: Word,
        side: Side,
    ) -> Result<Option<Runnable>, CellFault> {
        let cell = CellRef::decode(c);
        self.suspend(me)?;
        self.stats.parks += 1;
        self.stats.channel_waits += 1;
        if self.point(me)? {
            self.stats.cancels += 1;
            return Ok(Some(Runnable::Resume {
                cell: me,
                k,
                wake: CANCELLED,
            }));
        }
        // 生きた channel でない値は、閉じて空の channel として扱う。閉じているか進める channel の道は、1つの worker では
        // 速い道が確かめた状態のまま待ちに来るので届かない守りである
        let ready = match self.chan_mut(cell) {
            None => true,
            Some(chan) => {
                chan.closed
                    || match side {
                        Side::Send => chan.can_send(),
                        Side::Recv => !chan.items.is_empty(),
                    }
            }
        };
        if ready {
            return Ok(Some(Runnable::Resume {
                cell: me,
                k,
                wake: WOKE,
            }));
        }
        let task = self.task_mut(me)?;
        task.k = Some(k);
        task.wait = Some(Wait::Cell(cell));
        let chan = self
            .chan_mut(cell)
            .ok_or(CellFault::Internal("a channel is freed while a task parks"))?;
        let sender = match side {
            Side::Send => {
                chan.senders.push_back(me);
                None
            }
            Side::Recv => {
                chan.receivers.push_back(me);
                if chan.can_send() {
                    chan.senders.pop_front()
                } else {
                    None
                }
            }
        };
        sender.map(|s| self.wake(s, cell)).transpose()
    }

    /// 閉じて空の channel を放す。品物は残っていないので、預かる ID を退かせるだけである。
    fn settle<M: Mode>(&mut self, ctx: &mut Ctx<M>, cell: CellRef) -> Result<(), CellFault> {
        if self
            .chan_mut(cell)
            .is_some_and(|chan| chan.closed && chan.items.is_empty())
        {
            let chan = self.take_chan(cell)?;
            chan.depot.close(ctx)?;
        }
        Ok(())
    }

    /// run の終わりに残った channel を放す。残った品物を預かる ID の下で落としてから ID を退かせる。
    fn free_chan<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        cell: CellRef,
        depth_offset: usize,
    ) -> Result<(), CellFault> {
        let ChanCell {
            items, mut depot, ..
        } = self.take_chan(cell)?;
        for item in items {
            depot.discard(ctx, item, depth_offset)?;
        }
        depot.close(ctx)?;
        self.stats.channels_swept += 1;
        Ok(())
    }

    /// channel のセルを登録表から取り出して放す。
    fn take_chan(&mut self, cell: CellRef) -> Result<ChanCell, CellFault> {
        let chan = match self.slots.get_mut(cell.index as usize) {
            Some((generation, kind @ Some(CellKind::Chan(_))))
                if *generation == cell.generation =>
            {
                kind.take()
            }
            _ => None,
        };
        let Some(CellKind::Chan(chan)) = chan else {
            return Err(CellFault::Internal(
                "a channel reference names a freed cell",
            ));
        };
        self.release(cell.index);
        Ok(chan)
    }
}

/// channel の待ちの側。
#[derive(Clone, Copy)]
enum Side {
    Send,
    Recv,
}

fn raise(failure: Outcome) -> Raise {
    match failure {
        Outcome::Io { kind, message } => Raise::Io { kind, message },
        Outcome::Panic(message) => Raise::Panic(message),
        Outcome::Value(_) => unreachable!("a failure is not a value"),
    }
}

/// 受け手の決まらない値の預かり先。預かる構造 (channel のセル、タスクのセル) の論理的な worker ID を持ち、送り手は `publish` で値を
/// 預け、受け手は `receive` で受け取る (docs/spec/object-model.md の「論理的な worker ID」)。
pub(crate) struct Depot {
    /// 預かる ID。速いモードでは 0 である
    worker: u32,
}

impl Depot {
    /// 新しい預かる ID を出す。`new_chan` が1回呼ぶ。
    pub(crate) fn open<M: Mode>(ctx: &mut Ctx<M>) -> Depot {
        Depot {
            worker: unsafe { ctx.heap_mut() }.fork_worker(),
        }
    }

    /// ほかで出した ID (タスクのセルの ID) を預かる ID として使う。`close` は呼ぶ側が ID を退かせる所で呼ぶ。
    pub(crate) fn of(worker: u32) -> Depot {
        Depot { worker }
    }

    /// 値 `w` を預ける。送り手の ID のまま、構造に入れる前に呼ぶ。`Unr` の物体に共有の印を付け、`lin` の物体から rc 1 の
    /// 道で届く物体の持ち主を預かる ID にする (docs/spec/object-model.md の「`share`」)。語は変わらない。
    pub(crate) fn publish<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        w: Word,
    ) -> Result<Word, CellFault> {
        unsafe { ctx.heap_mut().share(w, self.worker) }?;
        Ok(w)
    }

    /// 構造から取り出した値 `w` を、今の worker の局所の値として受け取る。取り出した後、ほかに参照がない状態で、受け手の
    /// ID のまま呼ぶ (docs/spec/object-model.md の「`transfer_back`」)。語は変わらない。
    pub(crate) fn receive<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        w: Word,
    ) -> Result<Word, CellFault> {
        unsafe { ctx.heap_mut().transfer_back(w, self.worker) }?;
        Ok(w)
    }

    /// 受け取られない値 `w` を預かる ID の下で落とす。`decref` は即値も、破棄処理を持つ `Foreign` も受ける。
    pub(crate) fn discard<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        w: Word,
        depth_offset: usize,
    ) -> Result<(), CellFault> {
        with_worker(ctx, self.worker, depth_offset, |ctx| unsafe {
            ctx.heap_mut().decref(w)
        })?;
        Ok(())
    }

    /// 預かる ID を退かせる。値をすべて `discard` した後、セルを放す直前に1回呼ぶ。ID の物体が残っていれば
    /// `TransferLeak` を返す (`par` の join と同じ)。
    pub(crate) fn close<M: Mode>(self, ctx: &mut Ctx<M>) -> Result<(), CellFault> {
        unsafe { ctx.heap_mut() }.retire_worker(self.worker)?;
        Ok(())
    }
}

/// ほかのタスクの ID の物体を放すときの切り替え。`evv` を空にして ID `id` に替え、`f` の後で元の `evv`、worker、
/// 深さの起点 `depth_offset` に戻す。RC には触れない (docs/spec/object-model.md の「タスクの切り替え」)。
pub(crate) fn with_worker<M: Mode, T>(
    ctx: &mut Ctx<M>,
    id: u32,
    depth_offset: usize,
    f: impl FnOnce(&mut Ctx<M>) -> T,
) -> T {
    let prev = ctx.heap().worker();
    // 入れる `()` は即値で、取り出した `evv` は戻すときにそのまま返すので、参照の数は変わらない
    let evv = unsafe { ctx.switch_task(Word::UNIT, id, 0) };
    let result = f(ctx);
    let _unit = unsafe { ctx.switch_task(evv, prev, depth_offset) };
    result
}

/// タスクの底に着いた `exit n` を、終わりの欄に書く。`control.exit` を呼ぶのはここだけである。最初の書き手だけが
/// 真を返す。実行系は、`main` の底なら普通に戻り、ほかのタスクの底ならほかのタスクを動かさずに worker の入口へ戻る
/// (docs/spec/object-model.md の「実行の制御」)。
pub(crate) fn end_with_exit(control: &Control, n: i64) -> bool {
    control.exit(n)
}

// 輪の単体テストは子のモジュールに置かない。子のモジュールは親を `super` の道で読むが、このファイルは `std` と
// `eml_rt` だけを読む約束で、`crates/eml_vm/tests/tasks.rs` がそれを行ごとに確かめる

/// 根のスコープに、動いているタスクのセルを1つ置く。輪だけを確かめるので、ヒープの ID は持たない。
#[cfg(test)]
fn running_task(registry: &mut Registry, scope: CellRef) -> CellRef {
    registry.alloc(CellKind::Task(TaskCell {
        phase: Phase::Running,
        outcome: None,
        scope,
        worker: 0,
        spawner: 0,
        member_at: 0,
        waiters: Vec::new(),
        k: None,
        current: scope,
        wait: None,
        cancel: false,
        shield: 0,
        budget: BUDGET,
        run: None,
        detached: false,
    }))
}

#[cfg(test)]
fn resumed_cells(out: &[Runnable]) -> Vec<(CellRef, i64)> {
    out.iter()
        .map(|run| match run {
            Runnable::Resume { cell, wake, .. } => (*cell, *wake),
            _ => panic!("expected a resume"),
        })
        .collect()
}

#[test]
fn timers_with_the_same_deadline_expire_in_registration_order() {
    let mut registry = Registry::new(0);
    let scope = registry.open_root(None, None).unwrap();
    let first = running_task(&mut registry, scope);
    let second = running_task(&mut registry, scope);
    assert!(
        registry
            .park_time(3, 50, first, Word::UNIT)
            .unwrap()
            .is_none()
    );
    assert!(
        registry
            .park_time(3, 50, second, Word::UNIT)
            .unwrap()
            .is_none()
    );
    assert_eq!(registry.next_deadline(), Some(53));
    let mut out = Vec::new();
    registry.expire(52, &mut out).unwrap();
    assert!(out.is_empty());
    registry.expire(53, &mut out).unwrap();
    assert_eq!(resumed_cells(&out), [(first, WOKE), (second, WOKE)]);
    assert_eq!(registry.next_deadline(), None);
}

#[test]
fn unparking_a_sleeping_task_removes_its_timer() {
    let mut registry = Registry::new(0);
    let scope = registry.open_root(None, None).unwrap();
    let first = running_task(&mut registry, scope);
    let second = running_task(&mut registry, scope);
    registry.park_time(0, 50, first, Word::UNIT).unwrap();
    registry.park_time(0, 50, second, Word::UNIT).unwrap();
    registry.task_mut(first).unwrap().cancel = true;
    let mut out = Vec::new();
    registry.unpark(first, &mut out).unwrap();
    assert_eq!(resumed_cells(&out), [(first, CANCELLED)]);
    out.clear();
    registry.expire(50, &mut out).unwrap();
    assert_eq!(resumed_cells(&out), [(second, WOKE)]);
}
