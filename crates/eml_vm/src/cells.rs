//! `Async` のタスクとスコープと channel の意味の核 (docs/spec/vm.md の「`Async` のタスク」、「channel」)。VM の
//! `Task`、`Scheduler`、フレームを知らず、走らせる仕事を `Runnable` と不透明な `WaitToken` で返す。ネイティブのコードが
//! 同じ核を `extern "C"` のエントリから呼べるように、`std` と `eml_rt` だけに依存する。
//!
//! セルが持つ語は数えた参照で、VM が1回入れて1回出す。`Registry` はヒープに触れる `Drop` を持たないので、誤りで終わった
//! run と捨てた run では、`Scheduler` と一緒に触れずに捨てる。
//!
//! 不変条件: スコープの `members` と `nested` の項は、いつも生きているセルを指す。`waiters` の項は古くてもよく、
//! `wake_waiter` が `wait` で確かめる。`k.is_some()` と、`wait` が `Some(Cell(_))`、`Some(Time(_))`、`Some(Io(_))` の
//! どれかであることは同値である (`Ready` の継続は列に積んだ仕事が持つ)。`timers` の項 `key → t` があるのは、
//! `t` の `wait` が `Some(Time(key))` のときか、`Some(Io(op))` で記録 `op` の `timer` が `Some(key)` のときだけで、
//! 時間の待ちの登録と記録が覚えた鍵には、いつも項がある。タスクの今のスコープ `current` から親へたどる列は、そのタスクが開いたスコープだけからなり、加わったスコープ `scope` で終わる。入れ子の `run` の根の
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
//! 操作の記録の不変条件: `ops` の項は、完了がまだ届いていない記録だけを指す。記録の `waiter` が `t` を指して完了が
//! まだなら、`t` の `wait` は `Some(Io(その記録))` である。1つの token に記録は1つで、1つのタスクの待ちの登録は1つである。
//! 記録を放すのは、受け付けたタスクの `take_op` と `sweep_all` だけである。
//!
//! 取り消しは、印を付ける `mark` と受け箱の処理 `drain` が待ちを外す `unpark` の2段で行う。`CANCELLED` で再開する
//! 仕事をほかのタスクのために作るのは、`drain` と、取り消しを頼んだ操作の完了を当てる `complete` だけである。外からの印付けは `shield` の境目 `boundary` で止まる。
//! `cancel`、`flag`、`cancel_new` はどのタスクの操作からも書く。`current` と `run` は持ち主だけが書き、印を付ける
//! 操作が読む。スコープの `shield` と `boundary` は開くときに1回書き、後は読むだけである。タスクのセルの残りは、
//! そのタスクを動かすか受け箱を処理する持ち主だけが読み書きする (docs/spec/vm.md の「取り消しの届け方」)。

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use eml_rt::{
    Completion, Control, Ctx, Done, Fault, HandleBin, HeapError, IoDriver, IoHandle, Mode, Op,
    OpKey, RawBuf, Word,
};

/// 再開するタスクに渡す `Std.Async.Wake` の `Woke` のタグ (docs/spec/stdlib.md の「`Std.Async`」)。
pub(crate) const WOKE: i64 = 0;
/// `Cancelled` のタグ。
pub(crate) const CANCELLED: i64 = 1;
/// `IOErrorKind` の `Cancelled` のタグ。この種類で終わったタスクは失敗に数えない。
pub(crate) const IO_CANCELLED: i64 = 5;
/// 待たずに済む速い道の予算 (docs/spec/stdlib.md の「操作の予算」)。
const BUDGET: u32 = 128;
/// 使い切った添字の generation。番号には出さず、この generation の添字は空けない。一周させると、古い `Chan a` の値が
/// 別の型の品物を持つ channel を指しうるからである (docs/spec/vm.md の「セルと登録表」)。
const RETIRED: u32 = u32::MAX;

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
    /// タスク `cell` を始める。`closure` はエントリ `start_task` に渡す閉包で、仕事が参照を1つ持つ。
    Start { cell: CellRef, closure: Word },
    /// 待っていたタスク `cell` を、継続 `k` に `wake` を渡して再開する。`k` は仕事が参照を1つ持つ。
    Resume { cell: CellRef, k: Word, wake: i64 },
    /// driver の操作を待っていたタスク `cell` を、継続 `k` に `wake` と token `op` を渡して再開する。`k` と `op` は
    /// 仕事が参照を1つずつ持つ。
    ResumeIo {
        cell: CellRef,
        k: Word,
        wake: i64,
        op: Word,
    },
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
    /// 操作の記録が終わるのを待つ。記録の `waiter` がこのタスクを指す。
    Io(CellRef),
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
    Op(OpRecord),
}

/// driver に出した操作の記録 (docs/spec/vm.md の「操作の記録」)。driver は語を見ないので、操作に貸した記憶の物体と
/// ストリームは記録が持つ。語は `Depot` で公開せず、受け付けたタスクの ID `worker` の物体のまま持つ。
struct OpRecord {
    /// driver が振った鍵。`Registry::ops` の項の鍵と同じである。
    key: OpKey,
    kind: OpKind,
    phase: OpPhase,
    /// 受け付けたタスクの論理的な worker ID。そのタスクの外で記録の語に触れるときは、この ID に切り替える。
    worker: u32,
    /// 操作に貸した `Bytes`。受け付けから取り出しまで、その物体へのただ1つの参照である。
    buf: Option<Word>,
    /// 受け付けに渡されたストリームかリスナー (`accept`)。`op_stream` が取り戻すまで持つ。
    stream: Option<Word>,
    /// 完了の中身から集める地点で作った `NetListener` か `NetStream` の `Foreign`。取り出しの行が受け取るまで持つ。
    made: Option<Word>,
    /// 待つ間だけ持つ token。再開の仕事に移す。
    token: Option<Word>,
    /// 待っているタスク。
    waiter: Option<CellRef>,
    /// 待ちの期限の輪の鍵。期限が来るか、完了が届くか、取り消しで外れるまで持つ。
    timer: Option<TimerKey>,
    /// 待つタスクが取り消され、driver に取り消しを頼んだ。完了は `Cancelled` で再開する。
    cancelling: bool,
    /// 待ちの期限が来て、driver に取り消しを頼んだ。完了は `Woke` で再開し、誤りの結果は期限切れとして扱う
    /// (docs/spec/stdlib.md の「`Std.IoTest`」)。
    timed_out: bool,
}

/// 記録の操作の種類。読みの `Transfer` の完了だけが、外の書いた長さを buffer に確定する。
enum OpKind {
    Read,
    Write,
    /// プールの仕事。buffer は写した後は読まないので、一意でなくてよい。
    Job,
    /// `prefix` は誤りの文言の前に付ける `cannot listen on {host}:{port}` である。
    Listen {
        prefix: Box<str>,
    },
    Accept,
    /// `prefix` は `cannot connect to {host}:{port}` である。
    Connect {
        prefix: Box<str>,
    },
}

impl OpKind {
    /// `finish` が投げる IO の失敗の文言の前に付ける語 (docs/spec/stdlib.md の「`Std.Net`」)。
    fn prefix(&self) -> &str {
        match self {
            OpKind::Read => "cannot read the stream",
            OpKind::Write => "cannot write to the stream",
            OpKind::Job => "the job failed",
            OpKind::Listen { prefix } | OpKind::Connect { prefix } => prefix,
            OpKind::Accept => "cannot accept a connection",
        }
    }
}

enum OpPhase {
    /// 読みの1つ目の操作。端が読めるようになるのを driver が待っている。buffer はまだ貸していない。
    Probe,
    /// driver が記憶を読み書きしている。
    Transfer,
    /// 完了が届いた。2つ目は完了の `cancelled` で、driver の取り消しだけで終わったことを表す。登録表が期限か取り消しの
    /// ために `Probe` を終わらせたときも立てる。ハンドルを作る操作の結果は `Ok(0)` で、作ったものは記録の `made` にある。
    Done(io::Result<usize>, bool),
}

/// `take_op` が返す、終わった操作の結果。
pub(crate) struct Taken {
    pub(crate) result: io::Result<usize>,
    /// 記録が持っていた buffer。呼ぶ側が返すか落とす。
    pub(crate) buf: Option<Word>,
    /// 記録が持っていた、完了の中身から作った `Foreign`。呼ぶ側が返すか落とす。
    pub(crate) made: Option<Word>,
    kind: OpKind,
    /// 待つタスクが取り消され、結果によらず包みが取り消しを投げる。
    pub(crate) cancelling: bool,
    /// 待ちの期限が来た。誤りの結果は IO の失敗でなく期限切れである。
    pub(crate) timed_out: bool,
    /// 完了の `cancelled`。
    cancelled: bool,
}

impl Taken {
    /// `op_outcome` と同じ結果の番号。
    pub(crate) fn outcome(&self) -> i64 {
        outcome(&self.result, self.timed_out, self.cancelled)
    }

    /// 結果の番号が `OUTCOME_FAILED` で、取り消しを頼んでいない記録の、IO の失敗の誤りと文言 (docs/spec/stdlib.md の
    /// 「`Std.Net`」)。
    pub(crate) fn failure(&self) -> Option<(&io::Error, String)> {
        match &self.result {
            Err(e) if !self.cancelling && self.outcome() == OUTCOME_FAILED => {
                Some((e, format!("{}: {e}", self.kind.prefix())))
            }
            _ => None,
        }
    }
}

/// `op_outcome` の結果の番号 (docs/spec/vm.md の「操作の記録」)。成功の完了は期限と競ってもデータである。
pub(crate) const OUTCOME_OK: i64 = 0;
/// 期限の取り消しで終わった。
const OUTCOME_LATE: i64 = 1;
const OUTCOME_RESET: i64 = 2;
const OUTCOME_REFUSED: i64 = 3;
const OUTCOME_ADDR_IN_USE: i64 = 4;
const OUTCOME_TIMED_OUT: i64 = 5;
/// そのほかの誤り。`finish` が投げる。
const OUTCOME_FAILED: i64 = 6;

/// 終わった記録の結果の番号。取り消しを頼んだ記録は包みが番号を見ないので、ここでは分けない。期限に負けたとみなすのは、
/// 期限が来て、完了が取り消しだけで終わったときだけである。期限と競った本物の誤りは、その誤りを報告した syscall が
/// 消費しているので、種類のまま報告する (docs/spec/stdlib.md の「`Std.Net`」)。
fn outcome(result: &io::Result<usize>, timed_out: bool, cancelled: bool) -> i64 {
    match result {
        Ok(_) => OUTCOME_OK,
        Err(_) if timed_out && cancelled => OUTCOME_LATE,
        Err(e) => match e.kind() {
            io::ErrorKind::ConnectionReset
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted => OUTCOME_RESET,
            io::ErrorKind::ConnectionRefused => OUTCOME_REFUSED,
            io::ErrorKind::AddrInUse => OUTCOME_ADDR_IN_USE,
            io::ErrorKind::TimedOut => OUTCOME_TIMED_OUT,
            _ => OUTCOME_FAILED,
        },
    }
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

/// 受け箱の知らせ。落ちたハンドルの知らせはハンドルの `Drop` が、取り消しの知らせは `mark` が、シグナルの知らせは
/// 監視のスレッドが積む。
pub(crate) enum Notice {
    HandleDropped(CellRef),
    Cancel(CellRef),
    /// 取り出す前の token が落ちた。std の包みからは届かない。
    OpDropped(CellRef),
    /// 監視のスレッドが中継した保留のシグナル (`Control::INTERRUPT` と `Control::TERMINATE` のビット)。
    Signal(u32),
}

/// ほかの worker やハンドルの破棄処理が核へ知らせる受け箱。
#[derive(Default)]
pub(crate) struct Inbox {
    notices: Mutex<Vec<Notice>>,
}

impl Inbox {
    pub(crate) fn push(&self, notice: Notice) {
        self.notices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(notice);
    }

    pub(crate) fn take(&self) -> Vec<Notice> {
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

/// `Std.Async.OpId` の `Foreign` の中身。操作の記録を指す。
pub(crate) struct OpToken {
    op: CellRef,
    inbox: Arc<Inbox>,
    /// `take_op` が受け取った。受け取った token は記録をもう指さないので、落ちても知らせない。
    taken: bool,
}

impl Drop for OpToken {
    // `TaskHandle` と同じく、ヒープの `decref` の中で走るので知らせを積むだけにする
    fn drop(&mut self) {
        if !self.taken {
            self.inbox.push(Notice::OpDropped(self.op));
        }
    }
}

/// `Std.IoTest.Stream` の `Foreign` の中身。driver のハンドルを指す。`peer` は `pair` の2つ目の端をまだ分けていない
/// 間だけある。
pub(crate) struct IoStream {
    handle: IoHandle,
    peer: Option<IoHandle>,
    bin: Arc<HandleBin>,
}

impl Drop for IoStream {
    // ヒープの `decref` の中で走るので driver に触れず、driver が次に閉じるハンドルとして積むだけにする
    // (docs/spec/vm.md の「時計と driver」)
    fn drop(&mut self) {
        let mut dropped = self
            .bin
            .dropped
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        dropped.push(self.handle);
        dropped.extend(self.peer);
    }
}

/// `Std.Net.TcpListener` の `Foreign` の中身。driver のハンドルと、待ち受けるポートを持つ。
pub(crate) struct NetListener {
    handle: IoHandle,
    port: u16,
    bin: Arc<HandleBin>,
}

impl Drop for NetListener {
    // `IoStream` と同じく、driver に触れずに閉じるハンドルとして積むだけにする (docs/spec/vm.md の「時計と driver」)
    fn drop(&mut self) {
        bin_push(&self.bin, self.handle);
    }
}

/// `Std.Net.TcpStream` の `Foreign` の中身。driver のハンドルと、作ったときの相手の番地を持つ。
pub(crate) struct NetStream {
    handle: IoHandle,
    peer: SocketAddr,
    bin: Arc<HandleBin>,
}

impl Drop for NetStream {
    // `NetListener` と同じ
    fn drop(&mut self) {
        bin_push(&self.bin, self.handle);
    }
}

fn bin_push(bin: &HandleBin, handle: IoHandle) {
    bin.dropped
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(handle);
}

/// 登録表が数える仕事の回数。検査モードでなくても数えるが、`RunStats` に写すのは検査モードだけである
/// (docs/spec/vm.md の「`RunStats`」)。
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct TaskStats {
    /// `start` が作ったタスクのセルの数。
    pub(crate) tasks: u64,
    /// `park`、`ready`、`park_time`、`park_drain`、`park_send`、`park_recv`、`park_io`、`park_io_until` が渡された
    /// 継続の数。
    pub(crate) parks: u64,
    /// スコープの最初の失敗より後に終わった、失敗のタスクの数。
    pub(crate) dropped_failures: u64,
    /// 取り消しが届いた回数。待つ前に取り消しを見つけた `park`、`park_time`、`park_drain`、`park_send`、`park_recv`、
    /// `park_io`、`park_io_until`、受け箱の処理が外した待ちと取り消しを頼んだ操作の待ち、取り消しで再開する `yield_now`、真を返した
    /// `cancel_point`、偽を返した `claim_scope` を数える (docs/spec/vm.md の「`RunStats`」)。
    pub(crate) cancels: u64,
    /// 時間の輪に載せた登録の数。`ms > 0` で、地点で取り消されなかった `park_time` と `park_io_until` を数える。
    /// `park_io_until` は、操作がもう終わっていて輪に載せなかったときも数える。
    pub(crate) timers: u64,
    /// channel に品物を積んだ回数と取った回数の和。
    pub(crate) channel_ops: u64,
    /// `park_send` と `park_recv` が渡された継続の数。`parks` にも数える。
    pub(crate) channel_waits: u64,
    /// `sweep_all` が放した channel の数。
    pub(crate) channels_swept: u64,
    /// driver から取り出し、操作の記録に当てた完了の数。
    pub(crate) completions: u64,
}

/// 1回の実行のセルの表。実行ごとに作る (docs/spec/vm.md の「`Async` のタスク」)。
pub(crate) struct Registry {
    /// 添字ごとの generation と中身。空いた添字の generation は、次に出す番号のものである。
    slots: Vec<(u32, Option<CellKind>)>,
    free: Vec<u32>,
    pub(crate) inbox: Arc<Inbox>,
    pub(crate) stats: TaskStats,
    /// 時間の輪。項は時間を待つタスクのセルの番号だけで、語を持たない (docs/spec/vm.md の「時間の待ち」)。
    timers: BTreeMap<TimerKey, CellRef>,
    /// 次に登録するタイマーの `seq`。
    timer_seq: u64,
    /// `Std.Atomic.Counter` の表。添字は `Machine` が run をまたいで持つので、回復でも戻さない。
    pub(crate) counters: Vec<i64>,
    /// driver の鍵から操作の記録へ。完了を記録に当てる。
    ops: HashMap<OpKey, CellRef>,
    /// `complete` が driver から完了を受ける列。集めるたびに作らないように使い回す。
    completed: Vec<Completion>,
    /// 届けたときに待つタスクのいなかったシグナルのビット。集める地点の後で run を止める
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) unheard: u32,
}

impl Registry {
    /// `generations` は前の run までに使った添字の、次に出す generation である (`next_generations`)。前の run の
    /// `Scope` と `Chan a` の値がこの run のセルを指さないように、添字ごとに generation を続ける。
    pub(crate) fn new(generations: Vec<u32>) -> Registry {
        // 小さい添字から使うように、逆の順に積む
        let free = (0..generations.len())
            .rev()
            .filter(|&index| generations[index] != RETIRED)
            .map(|index| u32::try_from(index).expect("the cells fit in u32 positions"))
            .collect();
        Registry {
            slots: generations.into_iter().map(|g| (g, None)).collect(),
            free,
            inbox: Arc::new(Inbox::default()),
            stats: TaskStats::default(),
            timers: BTreeMap::new(),
            timer_seq: 0,
            counters: Vec::new(),
            ops: HashMap::new(),
            completed: Vec::new(),
            unheard: 0,
        }
    }

    /// いちばん早いタイマーの期限。輪が空なら `None` である。
    pub(crate) fn next_deadline(&self) -> Option<u64> {
        self.timers.first_key_value().map(|(key, _)| key.deadline)
    }

    /// 期限が `now_ms` までのタイマーを、期限の順、同じ期限なら登録の順に外し、待っていたタスクを再開する仕事を
    /// `out` に積む。輪の項と待ちの登録は、どちらが先に外れても他方を残さない (docs/spec/vm.md の「時間の待ち」)。
    /// 操作の待ちの期限は再開せず、driver に取り消しを頼む。driver が記憶を使い終わるまで buffer を返せないので、
    /// 再開は完了を当てる `complete` が行う。
    pub(crate) fn expire(
        &mut self,
        now_ms: u64,
        driver: &mut dyn IoDriver,
        out: &mut Vec<Runnable>,
    ) -> Result<(), CellFault> {
        while let Some(entry) = self.timers.first_entry() {
            let key = *entry.key();
            if key.deadline > now_ms {
                break;
            }
            let cell = entry.remove();
            let task = self.task_mut(cell)?;
            let parked = matches!(task.phase, Phase::Parked);
            if let (true, Some(Wait::Io(op))) = (parked, &task.wait) {
                let op = *op;
                let record = self.op_mut(op)?;
                if record.timer != Some(key) {
                    return Err(CellFault::Internal("a timer names no waiting task"));
                }
                record.timer = None;
                record.timed_out = true;
                driver.cancel(record.key);
                continue;
            }
            if !parked || !matches!(task.wait, Some(Wait::Time(k)) if k == key) {
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
                self.slots.push((0, None));
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

    /// generation を1つ進めて古い番号を見分け、添字を使い回せるようにする。進めた generation が `RETIRED` なら、
    /// 添字は空けずに退かせる。出した番号の generation は `RETIRED` より小さいので、足しても一周しない。
    fn release(&mut self, index: u32) {
        let slot = &mut self.slots[index as usize];
        slot.0 += 1;
        slot.1 = None;
        if slot.0 != RETIRED {
            self.free.push(index);
        }
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
            CellKind::Scope(_) | CellKind::Chan(_) | CellKind::Op(_) => Err(CellFault::Internal(
                "a task cell reference names another kind of cell",
            )),
        }
    }

    fn scope_mut(&mut self, cell: CellRef) -> Result<&mut ScopeCell, CellFault> {
        match self.cell_mut(cell)? {
            CellKind::Scope(scope) => Ok(scope),
            CellKind::Task(_) | CellKind::Chan(_) | CellKind::Op(_) => Err(CellFault::Internal(
                "a scope cell reference names another kind of cell",
            )),
        }
    }

    /// タスクの論理的な worker ID。
    pub(crate) fn worker(&mut self, cell: CellRef) -> Result<u32, CellFault> {
        Ok(self.task_mut(cell)?.worker)
    }

    /// 表の中の `Counter`。添字が表の外なのは、検査を通ったプログラムからは起きない。
    pub(crate) fn counter(&mut self, i: i64) -> Result<&mut i64, CellFault> {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.counters.get_mut(i))
            .ok_or(CellFault::Internal("a counter index is out of range"))
    }

    /// 次の run の `generations`。添字ごとに、この run までに出したどの generation よりも大きい。誤りで止まった run は
    /// セルを残すので、残ったセルの generation も1つ進める。`slots` は run の間に縮まない。
    pub(crate) fn next_generations(&self) -> Vec<u32> {
        self.slots
            .iter()
            .map(|(generation, kind)| generation + u32::from(kind.is_some()))
            .collect()
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
    /// `g` はエントリに渡す閉包である。新しい ID に渡す前に、`g` とまだ印のないセッションの根 `roots` に共有の印を付ける
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
            CellKind::Chan(_) | CellKind::Op(_) => {
                return Err(CellFault::Internal("a cell of the wrong kind"));
            }
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
            CellKind::Chan(_) | CellKind::Op(_) => {
                return Err(CellFault::Internal("a cell of the wrong kind"));
            }
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
    /// 知らせを積むために自分の run の受け箱を持つので、それを比べれば run を見分けられる。
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
            CellKind::Chan(_) | CellKind::Op(_) => {
                return Err(CellFault::Internal("a cell of the wrong kind"));
            }
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
    /// channel の待ち手の列の項はいつも生きているので、ここで外す。操作を待つタスクは、driver が記憶を使い終わるまで
    /// 再開できないので、待ちを外さずに取り消しを頼み、完了で `Cancelled` として再開する (docs/spec/vm.md の
    /// 「取り消しの届け方」)。
    fn unpark(
        &mut self,
        driver: &mut dyn IoDriver,
        cell: CellRef,
        out: &mut Vec<Runnable>,
    ) -> Result<(), CellFault> {
        let Some(CellKind::Task(task)) = self.live(cell) else {
            return Ok(());
        };
        if !matches!(task.phase, Phase::Parked)
            || !matches!(task.wait, Some(Wait::Cell(_) | Wait::Time(_) | Wait::Io(_)))
        {
            return Ok(());
        }
        let io = match task.wait {
            Some(Wait::Io(op)) => Some(op),
            _ => None,
        };
        if !self.point(cell)? {
            return Ok(());
        }
        if let Some(op) = io {
            let record = self.op_mut(op)?;
            if !record.cancelling {
                record.cancelling = true;
                // 期限がもう取り消しを頼んでいれば、頼み直さない
                if !record.timed_out {
                    driver.cancel(record.key);
                }
                // 輪の項を残すと、満期に `expire` が取り消した記録に期限が来た印を付ける
                if let Some(key) = record.timer.take() {
                    self.timers.remove(&key);
                }
                self.stats.cancels += 1;
            }
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
            Some(Wait::Ready | Wait::Io(_)) | None => {}
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
        driver: &mut dyn IoDriver,
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
                    Notice::Cancel(cell) => self.unpark(driver, cell, out)?,
                    // std の包みは受け付けから `park_io` まで投げうる呼び出しなしに token を移し、待つ間は記録が持ち、
                    // 取り出しは投げる前に受け取った印を付けるので、生きた記録の token は落ちない。放した記録への古い
                    // 知らせは、落ちたハンドルと同じく無視する
                    Notice::OpDropped(op) => {
                        if let Some(CellKind::Op(_)) = self.live(op) {
                            return Err(CellFault::Internal(
                                "an op token was dropped before its take",
                            ));
                        }
                    }
                    // シグナルを待つタスクはまだいないので、届けたシグナルはどれも聞く者がいない。届けたので保留から
                    // 下ろし、次の同じシグナルをハンドラが1回目として扱うようにする (docs/spec/vm.md の「上限と割り込み」)
                    Notice::Signal(bits) => {
                        self.unheard |= bits;
                        ctx.heap().control().settle(bits);
                    }
                }
            }
        }
    }

    /// run の終わりに、残ったセルをすべて放す。閉じていない channel と品物の残った channel もここで放す。その間に
    /// 積まれた知らせは受け箱ごと捨てる。driver が保留の操作を持たないことを前提にし、呼ぶ側が先に `quiesce` する
    /// (docs/spec/vm.md の「時計と driver」)。操作の記録の語は受け付けたタスクの ID の物体なので、タスクのセルが ID を
    /// 退かせる前に、記録を先に回って落とす (docs/spec/vm.md の「終わり」)。
    pub(crate) fn sweep_all<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        depth_offset: usize,
    ) -> Result<(), CellFault> {
        for index in 0..self.slots.len() {
            if matches!(self.slots[index].1, Some(CellKind::Op(_)))
                && let Some(CellKind::Op(record)) = self.slots[index].1.take()
            {
                // 預かる ID は記録のものではなく、タスクのセルか `main` のもので、`free_task` が退かせる
                let mut depot = Depot::of(record.worker);
                for w in [record.buf, record.stream, record.made, record.token]
                    .into_iter()
                    .flatten()
                {
                    depot.discard(ctx, w, depth_offset)?;
                }
                self.release(index as u32);
            }
        }
        self.ops.clear();
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
                Some(CellKind::Op(_)) | None => {}
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

/// driver の操作の記録 (docs/spec/vm.md の「操作の記録」)。受け付けの行が buffer とストリームを記録に移して
/// token を返し、`park_io` が待ち、集める地点の `complete` が完了を当てて再開し、取り出しの行が記録を放す。完了を
/// 受け取るのは集める地点だけで、driver の順に当てる。
impl Registry {
    fn op_mut(&mut self, op: CellRef) -> Result<&mut OpRecord, CellFault> {
        match self.cell_mut(op)? {
            CellKind::Op(record) => Ok(record),
            CellKind::Task(_) | CellKind::Scope(_) | CellKind::Chan(_) => Err(CellFault::Internal(
                "an op reference names another kind of cell",
            )),
        }
    }

    /// 借りた token `token` が指す記録。token の受け箱が登録表の受け箱と違えば、前の run の token である。
    fn token_op<M: Mode>(&self, ctx: &Ctx<M>, token: Word) -> Result<CellRef, CellFault> {
        // token は `OpId` の `Foreign` で、呼ぶ側が持つ唯一の参照である (`OpId` は `Lin`)
        let t = unsafe { &*ctx.heap().foreign_unique::<OpToken>(token)? };
        if !Arc::ptr_eq(&t.inbox, &self.inbox) {
            return Err(CellFault::Internal("an op token outlived its run"));
        }
        Ok(t.op)
    }

    /// `Std.IoTest.submit_write` と `Std.Net.submit_write`。`buf` と `stream` は呼ぶ側から移ってきた参照で、記録に移す。
    /// `buf` の内容をすべて書く操作を driver に出し、token を返す。`worker` は呼んだタスクの論理的な ID である。
    pub(crate) fn submit_write<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        driver: &mut dyn IoDriver,
        worker: u32,
        buf: Word,
        stream: Word,
    ) -> Result<Word, CellFault> {
        let buf = unique_bytes(ctx, buf)?;
        let handle = stream_handle(ctx, stream)?;
        // 記録が物体を生かし、一意に保ち、取り出すまで書き換えないので、driver は完了までこの位置を読める
        // (docs/spec/object-model.md の T10 の行)
        let bytes = unsafe { ctx.heap().bytes(buf)? };
        let raw = RawBuf {
            ptr: if bytes.is_empty() {
                std::ptr::NonNull::dangling().as_ptr()
            } else {
                bytes.cast::<u8>().cast_mut()
            },
            cap: bytes.len(),
        };
        let key = driver.submit(Op::Write { handle, buf: raw });
        Ok(self.record(
            ctx,
            OpRecord {
                key,
                kind: OpKind::Write,
                phase: OpPhase::Transfer,
                worker,
                buf: Some(buf),
                stream: Some(stream),
                made: None,
                token: None,
                waiter: None,
                timer: None,
                cancelling: false,
                timed_out: false,
            },
        ))
    }

    /// `Std.IoTest.submit_read` と `Std.Net.submit_read`。`buf` と `stream` を `submit_write` と同じく記録に移し、端が
    /// 読めるようになるのを待つ操作を driver に出す。buffer の容量はここでは足さず、読めるようになってから足して貸す。
    /// 何も来ない端の読みに記憶を持たせないためである (docs/spec/vm.md の「操作の記録」)。
    pub(crate) fn submit_read<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        driver: &mut dyn IoDriver,
        worker: u32,
        buf: Word,
        stream: Word,
    ) -> Result<Word, CellFault> {
        let buf = unique_bytes(ctx, buf)?;
        let handle = stream_handle(ctx, stream)?;
        let key = driver.submit(Op::Readable { handle });
        Ok(self.record(
            ctx,
            OpRecord {
                key,
                kind: OpKind::Read,
                phase: OpPhase::Probe,
                worker,
                buf: Some(buf),
                stream: Some(stream),
                made: None,
                token: None,
                waiter: None,
                timer: None,
                cancelling: false,
                timed_out: false,
            },
        ))
    }

    /// プールの仕事の受け付け。`buf` は呼ぶ側から移ってきた参照で、記録に移す。中身を `Vec<u8>` に写して `make` に
    /// 渡し、できた仕事を driver に出す。仕事は別のスレッドで動くので、ヒープの記憶を貸さない
    /// (docs/spec/vm.md の「操作の記録」)。
    pub(crate) fn submit_job<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        driver: &mut dyn IoDriver,
        worker: u32,
        buf: Word,
        make: impl FnOnce(Vec<u8>) -> Op,
    ) -> Result<Word, CellFault> {
        // 写すまでの間だけ読むので、共有の物体でもよい。仕事には写しを渡し、物体の位置は渡さない
        // (docs/spec/vm.md の「操作の記録」)
        let input = unsafe { (*ctx.heap().bytes(buf)?).to_vec() };
        let key = driver.submit(make(input));
        Ok(self.record(
            ctx,
            OpRecord {
                key,
                kind: OpKind::Job,
                phase: OpPhase::Transfer,
                worker,
                buf: Some(buf),
                stream: None,
                made: None,
                token: None,
                waiter: None,
                timer: None,
                cancelling: false,
                timed_out: false,
            },
        ))
    }

    /// `Std.Net.submit_listen`。`host` と `port` は呼ぶ側から移ってきた参照と即値で、`host` は読んでから手放す。
    /// `host` が IP アドレスの綴りでないか `port` が範囲の外なら、操作を出さずに IO の失敗の文言を返す
    /// (docs/spec/stdlib.md の「`Std.Net`」)。
    pub(crate) fn submit_listen<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        driver: &mut dyn IoDriver,
        worker: u32,
        host: Word,
        port: i64,
    ) -> Result<Result<Word, String>, CellFault> {
        let host = take_string(ctx, host)?;
        let prefix = format!("cannot listen on {host}:{port}");
        let Ok(ip) = host.parse::<IpAddr>() else {
            return Ok(Err(format!("{prefix}: the host is not an IP address")));
        };
        let Ok(port) = u16::try_from(port) else {
            return Ok(Err(format!("{prefix}: the port is out of range")));
        };
        let key = driver.submit(Op::Listen {
            addr: SocketAddr::new(ip, port),
        });
        let kind = OpKind::Listen {
            prefix: prefix.into(),
        };
        Ok(Ok(self.record(ctx, net_record(key, kind, worker, None))))
    }

    /// `Std.Net.submit_accept`。リスナー `listener` は呼ぶ側から移ってきた参照で、記録に移す。
    pub(crate) fn submit_accept<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        driver: &mut dyn IoDriver,
        worker: u32,
        listener: Word,
    ) -> Result<Word, CellFault> {
        // リスナーは `TcpListener` の `Foreign` で、呼ぶ側から移ってきた唯一の参照である (`TcpListener` は `Lin`)
        let handle = unsafe { (*ctx.heap().foreign_unique::<NetListener>(listener)?).handle };
        let key = driver.submit(Op::Accept { listener: handle });
        let record = net_record(key, OpKind::Accept, worker, Some(listener));
        Ok(self.record(ctx, record))
    }

    /// `Std.Net.submit_connect`。`submit_listen` と同じく `host` を読んで手放す。`host` は名前でもよく、driver が解決する。
    /// `port` が 1 から 65535 の外なら、操作を出さずに IO の失敗の文言を返す (docs/spec/stdlib.md の「`Std.Net`」)。
    pub(crate) fn submit_connect<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        driver: &mut dyn IoDriver,
        worker: u32,
        host: Word,
        port: i64,
    ) -> Result<Result<Word, String>, CellFault> {
        let host = take_string(ctx, host)?;
        let prefix = format!("cannot connect to {host}:{port}");
        let port = match u16::try_from(port) {
            Ok(port) if port > 0 => port,
            _ => return Ok(Err(format!("{prefix}: the port is out of range"))),
        };
        let key = driver.submit(Op::Connect { host, port });
        let kind = OpKind::Connect {
            prefix: prefix.into(),
        };
        Ok(Ok(self.record(ctx, net_record(key, kind, worker, None))))
    }

    /// 記録を表に入れ、鍵の項を足し、token を作る。
    fn record<M: Mode>(&mut self, ctx: &mut Ctx<M>, record: OpRecord) -> Word {
        let key = record.key;
        let op = self.alloc(CellKind::Op(record));
        self.ops.insert(key, op);
        // token を確保するだけで、`Ctx` の持つ語には触れない。`OpId` は `Lin` なので `lin` ビットを立てる
        // (docs/spec/vm.md の「操作の記録」)
        unsafe { ctx.heap_mut() }.alloc_foreign(
            OpToken {
                op,
                inbox: Arc::clone(&self.inbox),
                taken: false,
            },
            true,
        )
    }

    /// タスク `me` が、継続 `k` で token `token` の操作が終わるまで待つ。`token` は呼ぶ側から移ってきた参照で、待つ間は
    /// 記録が持ち、再開の仕事に移す。終わっていれば、すぐ再開する仕事を返す。取り消しの地点で、終わっていなければ
    /// driver に取り消しを頼んで完了を待つ。driver が記憶を使い終わるまで、buffer を返せないためである。取り消しの
    /// 地点なら、終わっていてもいなくても、記録に取り消しを頼んだ印を付ける。
    pub(crate) fn park_io<M: Mode>(
        &mut self,
        ctx: &Ctx<M>,
        driver: &mut dyn IoDriver,
        token: Word,
        me: CellRef,
        k: Word,
    ) -> Result<Option<Runnable>, CellFault> {
        self.park_op(ctx, driver, token, me, k, None)
    }

    /// `park_io` に `ms` ミリ秒の期限を付けた待ち。`now_ms` は切り上げて読んだ今の時刻で、`ms` が 0 以下なら読まない。
    /// 期限は実際に待つときだけ付ける。0 以下は過ぎた期限なので、すぐ driver に取り消しを頼む。期限が来ても再開せず、
    /// 取り消した操作の完了が `Woke` で再開する (docs/spec/vm.md の「時間の待ち」)。
    #[expect(
        clippy::too_many_arguments,
        reason = "the operands of `park_io` and the deadline"
    )]
    pub(crate) fn park_io_until<M: Mode>(
        &mut self,
        ctx: &Ctx<M>,
        driver: &mut dyn IoDriver,
        now_ms: u64,
        ms: i64,
        token: Word,
        me: CellRef,
        k: Word,
    ) -> Result<Option<Runnable>, CellFault> {
        self.park_op(ctx, driver, token, me, k, Some((now_ms, ms)))
    }

    fn park_op<M: Mode>(
        &mut self,
        ctx: &Ctx<M>,
        driver: &mut dyn IoDriver,
        token: Word,
        me: CellRef,
        k: Word,
        until: Option<(u64, i64)>,
    ) -> Result<Option<Runnable>, CellFault> {
        let op = self.token_op(ctx, token)?;
        if self.op_mut(op)?.waiter.is_some() {
            return Err(CellFault::Internal("an op is awaited twice"));
        }
        self.suspend(me)?;
        self.stats.parks += 1;
        let cancelled = self.point(me)?;
        if cancelled {
            self.stats.cancels += 1;
        }
        // 正の期限は、操作がもう終わっていて輪に載せないときも数える。終わったかどうかはスケジュールで変わるので、
        // 数をプログラムだけで決めるためである (docs/spec/vm.md の「`RunStats`」)
        if matches!(until, Some((_, ms)) if ms > 0) && !cancelled {
            self.stats.timers += 1;
        }
        let record = self.op_mut(op)?;
        // `Cancelled` で再開する記録には、いつも取り消しの印を付ける。取り出しの行は印で中断を見分け、結果の誤りを
        // 投げずに包みへ返すので、包みが結果によらず `Cancelled` を投げる (docs/spec/stdlib.md の「`Std.IoTest`」)
        record.cancelling |= cancelled;
        if matches!(record.phase, OpPhase::Done(..)) {
            let wake = if cancelled { CANCELLED } else { WOKE };
            return Ok(Some(Runnable::ResumeIo {
                cell: me,
                k,
                wake,
                op: token,
            }));
        }
        if cancelled {
            driver.cancel(record.key);
        }
        record.waiter = Some(me);
        record.token = Some(token);
        match until {
            Some((_, ms)) if !cancelled && ms <= 0 => {
                record.timed_out = true;
                driver.cancel(record.key);
            }
            Some((now_ms, ms)) if !cancelled => {
                // 期限は `park_time` と同じく飽和させる
                let key = TimerKey {
                    deadline: now_ms.saturating_add(ms as u64),
                    seq: self.timer_seq,
                };
                self.timer_seq += 1;
                self.timers.insert(key, me);
                self.op_mut(op)?.timer = Some(key);
            }
            _ => {}
        }
        let task = self.task_mut(me)?;
        task.k = Some(k);
        task.wait = Some(Wait::Io(op));
        Ok(None)
    }

    /// 集める地点で、driver の完了を driver の順に記録へ当て、待つタスクを再開する仕事を `out` に積む。記録がなければ
    /// driver に触れない。読みの `Probe` の完了は、buffer を貸して `Transfer` の操作を新しい鍵で出し直し、待ちを続ける。
    /// 記録の語は受け付けたタスクの物体なので、今のタスクが誰でも、その ID に切り替えてから触れる。
    pub(crate) fn complete<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        driver: &mut dyn IoDriver,
        depth_offset: usize,
        out: &mut Vec<Runnable>,
    ) -> Result<(), CellFault> {
        if self.ops.is_empty() {
            return Ok(());
        }
        let mut completed = std::mem::take(&mut self.completed);
        driver.completions(&mut completed);
        for Completion {
            key,
            result,
            cancelled,
        } in completed.drain(..)
        {
            self.stats.completions += 1;
            let op = self
                .ops
                .remove(&key)
                .ok_or(CellFault::Internal("a completion names no op"))?;
            let mut cancelled = cancelled;
            let mut made = None;
            let record = self.op_mut(op)?;
            let result = match (&record.phase, &record.kind, result) {
                (OpPhase::Done(..), _, _) => {
                    return Err(CellFault::Internal("an op completes twice"));
                }
                // 取り消しか期限で driver に取り消しを頼んだ読みには、もう buffer を貸さない。取り消しなら包みは結果に
                // よらず取り消しを投げ、期限なら `None` を返す。読んだバイトはないので、取り消しで終わった完了と同じに扱う
                (OpPhase::Probe, _, _) if record.cancelling || record.timed_out => {
                    cancelled = true;
                    Err(io::Error::other("cancelled"))
                }
                (OpPhase::Probe, _, Err(e)) => Err(e),
                (OpPhase::Probe, _, Ok(Done::Size(_))) => {
                    let (worker, buf, stream) = (record.worker, record.buf, record.stream);
                    let (Some(buf), Some(stream)) = (buf, stream) else {
                        return Err(CellFault::Internal("a read op has no buffer or stream"));
                    };
                    let (buf, handle, raw) = with_worker(ctx, worker, depth_offset, |ctx| {
                        lend_read_room(ctx, buf, stream)
                    })?;
                    let key = driver.submit(Op::Read { handle, buf: raw });
                    let record = self.op_mut(op)?;
                    record.buf = Some(buf);
                    record.key = key;
                    record.phase = OpPhase::Transfer;
                    self.ops.insert(key, op);
                    continue;
                }
                // 取り消しを頼んだ読みも、外が書いたバイトは確定してから buffer を返す
                (OpPhase::Transfer, OpKind::Read, Ok(Done::Size(n))) => {
                    let (worker, buf) = (record.worker, record.buf);
                    let buf = buf.ok_or(CellFault::Internal("a read op has no buffer"))?;
                    // 記録は貸した後も buffer の唯一の参照を持ち、書き換えていない。driver は `Ok(n)` で `n` バイトを
                    // 書き終えている (docs/spec/object-model.md の T10 の行、docs/spec/vm.md の「操作の記録」)
                    with_worker(ctx, worker, depth_offset, |ctx| unsafe {
                        ctx.heap_mut().bytes_commit(buf, n)
                    })?;
                    Ok(n)
                }
                (OpPhase::Transfer, OpKind::Write | OpKind::Job, Ok(Done::Size(n))) => Ok(n),
                // 完了の中身のハンドルは、ここで記録のタスクの物体にする。取り消しを頼んだ記録でも作り、取り出しか
                // `finish` が落とすと、ハンドルは bin を通して閉じる (docs/spec/vm.md の「操作の記録」)
                (OpPhase::Transfer, OpKind::Listen { .. }, Ok(Done::Listener { handle, port })) => {
                    let listener = NetListener {
                        handle,
                        port,
                        bin: driver.bin(),
                    };
                    made = Some(with_worker(ctx, record.worker, depth_offset, |ctx| {
                        // 確保するだけで、`Ctx` の持つ語には触れない。`TcpListener` は `Lin` である
                        unsafe { ctx.heap_mut() }.alloc_foreign(listener, true)
                    }));
                    Ok(0)
                }
                (
                    OpPhase::Transfer,
                    OpKind::Accept | OpKind::Connect { .. },
                    Ok(Done::Stream { handle, peer }),
                ) => {
                    let stream = NetStream {
                        handle,
                        peer,
                        bin: driver.bin(),
                    };
                    made = Some(with_worker(ctx, record.worker, depth_offset, |ctx| {
                        // `NetListener` と同じ。`TcpStream` は `Lin` である
                        unsafe { ctx.heap_mut() }.alloc_foreign(stream, true)
                    }));
                    Ok(0)
                }
                (OpPhase::Transfer, _, Err(e)) => Err(e),
                (OpPhase::Probe | OpPhase::Transfer, _, Ok(_)) => {
                    return Err(CellFault::Internal(
                        "an op completes with another kind of result",
                    ));
                }
            };
            let record = self.op_mut(op)?;
            record.made = made;
            record.phase = OpPhase::Done(result, cancelled);
            // 終わった操作の期限は要らない。輪の項を残すと、満期に `expire` が待たないタスクを名指す
            let timer = record.timer.take();
            let waiter = record.waiter;
            let wake = if record.cancelling { CANCELLED } else { WOKE };
            let token = record.token.take();
            if let Some(key) = timer {
                self.timers.remove(&key);
            }
            let Some(t) = waiter else {
                continue;
            };
            let token = token.ok_or(CellFault::Internal("a waited op has no token"))?;
            let task = self.task_mut(t)?;
            if !matches!(task.wait, Some(Wait::Io(o)) if o == op) {
                return Err(CellFault::Internal("an op names no waiting task"));
            }
            task.wait = None;
            let k = task
                .k
                .take()
                .ok_or(CellFault::Internal("a parked task has no continuation"))?;
            out.push(Runnable::ResumeIo {
                cell: t,
                k,
                wake,
                op: token,
            });
        }
        self.completed = completed;
        Ok(())
    }

    /// `Std.IoTest.op_stream`、`Std.Net.op_stream` と `op_listener`。借りた token の終わった記録から、受け付けに渡した
    /// ストリームかリスナーを移し出す。
    pub(crate) fn op_stream<M: Mode>(
        &mut self,
        ctx: &Ctx<M>,
        token: Word,
    ) -> Result<Word, CellFault> {
        let op = self.token_op(ctx, token)?;
        let record = self.op_mut(op)?;
        if !matches!(record.phase, OpPhase::Done(..)) {
            return Err(CellFault::Internal("an op is taken before its completion"));
        }
        record
            .stream
            .take()
            .ok_or(CellFault::Internal("an op has no stream to give back"))
    }

    /// `Std.IoTest.op_timed_out`。借りた token の終わった記録が、期限で誤りになったか。期限と競って届いたデータは
    /// データである (docs/spec/stdlib.md の「`Std.IoTest`」)。
    pub(crate) fn op_timed_out<M: Mode>(
        &mut self,
        ctx: &Ctx<M>,
        token: Word,
    ) -> Result<bool, CellFault> {
        let op = self.token_op(ctx, token)?;
        let record = self.op_mut(op)?;
        match &record.phase {
            OpPhase::Done(result, _) => Ok(record.timed_out && result.is_err()),
            OpPhase::Probe | OpPhase::Transfer => {
                Err(CellFault::Internal("an op is taken before its completion"))
            }
        }
    }

    /// `Std.Net.op_outcome`。借りた token の終わった記録の結果の番号 (docs/spec/vm.md の「操作の記録」)。
    pub(crate) fn op_outcome<M: Mode>(
        &mut self,
        ctx: &Ctx<M>,
        token: Word,
    ) -> Result<i64, CellFault> {
        let op = self.token_op(ctx, token)?;
        let record = self.op_mut(op)?;
        match &record.phase {
            OpPhase::Done(result, cancelled) => Ok(outcome(result, record.timed_out, *cancelled)),
            OpPhase::Probe | OpPhase::Transfer => {
                Err(CellFault::Internal("an op is taken before its completion"))
            }
        }
    }

    /// `Std.Net.take_listener` と `take_stream`。token を消費して記録を放し、完了の中身から作った `Foreign` を返す。
    /// 結果の番号が `OUTCOME_OK` の記録だけを取り出す。`listener` はリスナーを作る操作 (`listen`) の記録を求める。
    pub(crate) fn take_made<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        token: Word,
        worker: u32,
        listener: bool,
    ) -> Result<Word, CellFault> {
        let taken = self.take_op(ctx, token, worker)?;
        if taken.outcome() != OUTCOME_OK {
            return Err(CellFault::Internal("an op is taken after it failed"));
        }
        let kind_matches = if listener {
            matches!(taken.kind, OpKind::Listen { .. })
        } else {
            matches!(taken.kind, OpKind::Accept | OpKind::Connect { .. })
        };
        match taken.made {
            Some(made) if kind_matches => Ok(made),
            _ => Err(CellFault::Internal("an op is taken as another kind of op")),
        }
    }

    /// `Std.Net.take_read`。token を消費して記録を放し、読んだバイトを足した buffer を返す。結果の番号が `OUTCOME_OK` の
    /// 読みの記録だけを取り出す (docs/spec/stdlib.md の「`Std.Net`」)。
    pub(crate) fn take_read<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        token: Word,
        worker: u32,
    ) -> Result<Word, CellFault> {
        let taken = self.take_op(ctx, token, worker)?;
        if taken.outcome() != OUTCOME_OK {
            return Err(CellFault::Internal("an op is taken after it failed"));
        }
        match taken.buf {
            Some(buf) if matches!(taken.kind, OpKind::Read) => Ok(buf),
            _ => Err(CellFault::Internal("an op is taken as another kind of op")),
        }
    }

    /// 取り出しの行。token を消費して終わった記録を放し、結果と buffer を返す。残ったストリームは落とす。取り出すのは
    /// 受け付けたタスク (`worker`) だけである。記録の語はそのタスクの物体なので、切り替えずに受け取れる。
    pub(crate) fn take_op<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        token: Word,
        worker: u32,
    ) -> Result<Taken, CellFault> {
        let op = self.token_op(ctx, token)?;
        let record = self.op_mut(op)?;
        if !matches!(record.phase, OpPhase::Done(..)) {
            return Err(CellFault::Internal("an op is taken before its completion"));
        }
        if record.worker != worker {
            return Err(CellFault::Internal("an op is taken by another task"));
        }
        let Some(CellKind::Op(record)) = self.slots[op.index as usize].1.take() else {
            unreachable!("the op cell is live");
        };
        self.release(op.index);
        // token は呼ぶ側から移ってきた唯一の参照で、ストリームは記録が持っていた参照である。どちらも受け付けた
        // タスクの ID の物体で、上でそのタスクだと確かめた。操作は終わっているので、driver はもう記憶に触れない
        // (docs/spec/vm.md の「操作の記録」)
        unsafe {
            (*ctx.heap().foreign_unique::<OpToken>(token)?).taken = true;
            ctx.heap_mut().decref(token)?;
        }
        if let Some(stream) = record.stream {
            unsafe { ctx.heap_mut().decref(stream) }?;
        }
        let OpPhase::Done(result, cancelled) = record.phase else {
            unreachable!("the phase is checked above");
        };
        Ok(Taken {
            result,
            buf: record.buf,
            made: record.made,
            kind: record.kind,
            cancelling: record.cancelling,
            timed_out: record.timed_out,
            cancelled,
        })
    }
}

/// buffer を持たない `Std.Net` の操作の記録。どれも driver が記憶を使わないので、段階は `Transfer` から始める。
fn net_record(key: OpKey, kind: OpKind, worker: u32, stream: Option<Word>) -> OpRecord {
    OpRecord {
        key,
        kind,
        phase: OpPhase::Transfer,
        worker,
        buf: None,
        stream,
        made: None,
        token: None,
        waiter: None,
        timer: None,
        cancelling: false,
        timed_out: false,
    }
}

/// 呼ぶ側から移ってきた `String` の参照 `w` の中身を写して、参照を手放す。
fn take_string<M: Mode>(ctx: &mut Ctx<M>, w: Word) -> Result<String, CellFault> {
    // `w` は呼ぶ側が持つ今の worker の参照で、`Ctx` の持つ語には触れない。中身は写し終えてから手放す
    let text = unsafe { (*ctx.heap().string(w)?).to_owned() };
    unsafe { ctx.heap_mut().decref(w)? };
    Ok(text)
}

/// `Std.Net.local_port`。借りたリスナーが待ち受けるポート。
pub(crate) fn local_port<M: Mode>(ctx: &Ctx<M>, listener: Word) -> Result<i64, CellFault> {
    // リスナーは呼ぶ側が持つ唯一の参照で、借りて読むだけである (`TcpListener` は `Lin`)
    Ok(i64::from(unsafe {
        (*ctx.heap().foreign_unique::<NetListener>(listener)?).port
    }))
}

/// `Std.Net.peer_addr`。借りたストリームの相手の番地を、Rust の `SocketAddr` の表示の形の新しい文字列にする。
pub(crate) fn peer_addr<M: Mode>(ctx: &mut Ctx<M>, stream: Word) -> Result<Word, CellFault> {
    // ストリームは呼ぶ側が持つ唯一の参照で、借りて読むだけである (`TcpStream` は `Lin`)
    let peer = unsafe { (*ctx.heap().foreign_unique::<NetStream>(stream)?).peer };
    Ok(ctx.fresh_string(&peer.to_string()))
}

/// ストリーム `stream` の driver のハンドル。`Std.Net.TcpStream` と `Std.IoTest.Stream` のどちらの `Foreign` でもよい。
/// ストリームは呼ぶ側から移ってきた参照か記録が持つ参照で、持ち主が返すか落とす。
pub(crate) fn stream_handle<M: Mode>(ctx: &Ctx<M>, stream: Word) -> Result<IoHandle, CellFault> {
    // ストリームは持ち主のただ1つの参照である (どちらの型も `Lin`。docs/spec/vm.md の「操作の記録」)。中身の型が
    // 違えば `WrongKind` で、もう一方の型で読み直す
    match unsafe { ctx.heap().foreign_unique::<NetStream>(stream) } {
        Ok(s) => Ok(unsafe { (*s).handle }),
        Err(HeapError::WrongKind) => {
            Ok(unsafe { (*ctx.heap().foreign_unique::<IoStream>(stream)?).handle })
        }
        Err(e) => Err(e.into()),
    }
}

/// `Std.IoTest.open_pair` が作る最初の端。`peer` は2つ目の端で、`peer` の行が分けるまで同じ物体が持つ。
pub(crate) fn open_stream<M: Mode>(
    ctx: &mut Ctx<M>,
    handle: IoHandle,
    peer: Option<IoHandle>,
    bin: Arc<HandleBin>,
) -> Word {
    // 端を確保するだけで、`Ctx` の持つ語には触れない。`Stream` は `Lin` なので `lin` ビットを立てる
    // (docs/spec/vm.md の「操作の記録」)
    unsafe { ctx.heap_mut() }.alloc_foreign(IoStream { handle, peer, bin }, true)
}

/// `Std.IoTest.peer`。借りた端 `stream` から2つ目の端を分けて、新しい物体にする。
pub(crate) fn peer<M: Mode>(ctx: &mut Ctx<M>, stream: Word) -> Result<Word, CellFault> {
    // 端は `Stream` の `Foreign` で、呼ぶ側が持つ唯一の参照である (`Stream` は `Lin`)
    let s = unsafe { &mut *ctx.heap().foreign_unique::<IoStream>(stream)? };
    let handle = s
        .peer
        .take()
        .ok_or(CellFault::Internal("a stream pair is split twice"))?;
    let bin = Arc::clone(&s.bin);
    Ok(open_stream(ctx, handle, None, bin))
}

/// 読みが1回に足す空きのバイト数 (docs/spec/vm.md の「操作の記録」)。
const READ_ROOM: usize = 65_536;

/// 読めるようになった端 `stream` の読みに、記録の buffer `buf` の長さの後ろの空きを、ちょうど `READ_ROOM` バイト貸す。
/// 空きが `READ_ROOM` より少なければ、長さに `READ_ROOM` を足した容量の新しい物体に中身を写し、元を手放す。1回の読みが
/// 足すバイトを、buffer の空きによらず `READ_ROOM` までにするためである (docs/spec/stdlib.md の「`Std.Net`」)。返すのは
/// 記録が持つ buffer の語、端のハンドル、貸した記憶である。記録の worker ID に切り替えた下で呼ぶ。`HeapError` は検査
/// モードの前提の違反で、driver に出す前に返すので、貸したままの記憶は残らない。確保量の上限は誤りにせず、次の安全点で
/// 止まる。
fn lend_read_room<M: Mode>(
    ctx: &mut Ctx<M>,
    buf: Word,
    stream: Word,
) -> Result<(Word, IoHandle, RawBuf), CellFault> {
    let handle = stream_handle(ctx, stream)?;
    let heap = unsafe { ctx.heap_mut() };
    // 貸すことは状態を変えないので、空きを測るのにも使える
    let room = unsafe { heap.bytes_lend(buf)? }.cap;
    let buf = if room < READ_ROOM {
        let content = unsafe { heap.bytes(buf)? };
        let len = content.len();
        let mut fresh = heap.alloc_bytes_room(len + READ_ROOM);
        if len > 0 {
            // `content` は `fresh` と別の物体の中身で、`fresh` を作っても動かない
            fresh = unsafe { heap.bytes_extend(fresh, &*content)? };
        }
        unsafe { heap.decref(buf)? };
        fresh
    } else {
        buf
    };
    // 記録が物体を生かし、一意に保ち、完了を受け取るまで書き換えないので、driver はこの空きに書ける
    // (docs/spec/object-model.md の T10 の行)
    let raw = unsafe { heap.bytes_lend(buf)? };
    let raw = RawBuf {
        ptr: raw.ptr,
        cap: READ_ROOM,
    };
    Ok((buf, handle, raw))
}

/// 呼ぶ側から移ってきた `Bytes` の参照 `w` を、一意な物体の参照にする。共有か不死の物体と、ほかに参照のある物体は、
/// 同じ中身の新しい物体に写して元を手放す。driver が読む間、ほかの参照から書き換えられないためである
/// (docs/spec/vm.md の「操作の記録」)。
fn unique_bytes<M: Mode>(ctx: &mut Ctx<M>, w: Word) -> Result<Word, CellFault> {
    // `w` は呼ぶ側が持つ今の worker の参照で、`Ctx` の持つ語には触れない。中身は写し終えてから手放す
    // (docs/spec/vm.md の「操作の記録」)
    let heap = unsafe { ctx.heap_mut() };
    if unsafe { heap.is_unique(w) }? {
        return Ok(w);
    }
    let content = unsafe { &*heap.bytes(w)? }.to_vec();
    let copy = heap.alloc_bytes(&content);
    unsafe { heap.decref(w) }?;
    Ok(copy)
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
/// 真を返す。実行系は、`main` の底なら普通に戻り、ほかのタスクの底ならほかのタスクを動かさずに worker のエントリへ戻る
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
    let mut registry = Registry::new(Vec::new());
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
    registry
        .expire(52, &mut NoDriver::default(), &mut out)
        .unwrap();
    assert!(out.is_empty());
    registry
        .expire(53, &mut NoDriver::default(), &mut out)
        .unwrap();
    assert_eq!(resumed_cells(&out), [(first, WOKE), (second, WOKE)]);
    assert_eq!(registry.next_deadline(), None);
}

#[test]
fn unparking_a_sleeping_task_removes_its_timer() {
    let mut registry = Registry::new(Vec::new());
    let scope = registry.open_root(None, None).unwrap();
    let first = running_task(&mut registry, scope);
    let second = running_task(&mut registry, scope);
    registry.park_time(0, 50, first, Word::UNIT).unwrap();
    registry.park_time(0, 50, second, Word::UNIT).unwrap();
    registry.task_mut(first).unwrap().cancel = true;
    let mut out = Vec::new();
    registry
        .unpark(&mut NoDriver::default(), first, &mut out)
        .unwrap();
    assert_eq!(resumed_cells(&out), [(first, CANCELLED)]);
    out.clear();
    registry
        .expire(50, &mut NoDriver::default(), &mut out)
        .unwrap();
    assert_eq!(resumed_cells(&out), [(second, WOKE)]);
}

#[test]
fn a_slot_retires_instead_of_wrapping_its_generation() {
    let mut registry = Registry::new(vec![RETIRED - 2]);
    let scope = registry.open_root(None, None).unwrap();
    assert_eq!(
        scope,
        CellRef {
            index: 0,
            generation: RETIRED - 2
        }
    );
    registry.release(scope.index);
    let last = registry.open_root(None, None).unwrap();
    assert_eq!(
        last,
        CellRef {
            index: 0,
            generation: RETIRED - 1
        }
    );
    registry.release(last.index);
    // 進めた generation が一周の手前に来た添字は空けないので、次のセルは新しい添字に置く
    let next = registry.open_root(None, None).unwrap();
    assert_eq!(
        next,
        CellRef {
            index: 1,
            generation: 0
        }
    );
    assert!(registry.live(scope).is_none() && registry.live(last).is_none());
    // 退いた添字は次の run でも使わず、ほかの添字は generation を続ける
    assert_eq!(registry.next_generations(), [RETIRED, 1]);
    let mut later = Registry::new(registry.next_generations());
    let reused = later.open_root(None, None).unwrap();
    assert_eq!(
        reused,
        CellRef {
            index: 1,
            generation: 1
        }
    );
    assert_eq!(
        later.open_root(None, None).unwrap(),
        CellRef {
            index: 2,
            generation: 0
        }
    );
}

/// 操作を受け付けるだけで、テストが積んだ完了だけを返す driver。核の単体テストが driver の crate に頼らないための
/// ものである。読みに貸された記憶を覚え、テストが外の書き手としてそこに書く。
#[cfg(test)]
#[derive(Default)]
struct NoDriver {
    next_key: OpKey,
    cancelled: Vec<OpKey>,
    done: Vec<Completion>,
    lent: Vec<RawBuf>,
}

#[cfg(test)]
struct NoWaker;

#[cfg(test)]
impl eml_rt::IoWaker for NoWaker {
    fn wake(&self) {}
}

#[cfg(test)]
impl IoDriver for NoDriver {
    fn now(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }

    fn wait(&mut self, _deadline_ms: Option<u64>) -> io::Result<bool> {
        Ok(false)
    }

    fn submit(&mut self, op: Op) -> OpKey {
        if let Op::Read { buf, .. } = op {
            self.lent.push(buf);
        }
        self.next_key += 1;
        self.next_key
    }

    fn cancel(&mut self, key: OpKey) {
        self.cancelled.push(key);
    }

    fn completions(&mut self, out: &mut Vec<Completion>) {
        out.append(&mut self.done);
    }

    fn pending(&self) -> usize {
        0
    }

    fn waker(&self) -> Arc<dyn eml_rt::IoWaker> {
        Arc::new(NoWaker)
    }

    fn bin(&self) -> Arc<HandleBin> {
        Arc::default()
    }

    fn pair(&mut self) -> io::Result<(IoHandle, IoHandle)> {
        Ok((0, 1))
    }

    fn shutdown_write(&mut self, _handle: IoHandle) -> io::Result<()> {
        Ok(())
    }

    fn set_nodelay(&mut self, _handle: IoHandle, _on: bool) -> io::Result<()> {
        Ok(())
    }

    fn close(&mut self, _handle: IoHandle) {}

    fn quiesce(&mut self) {}
}

/// 出力を捨てる Host の上の、検査モードの `Ctx`。
#[cfg(test)]
fn test_ctx() -> Ctx<eml_rt::Checked> {
    let host = eml_rt::StdHost::new(
        Box::new(io::sink()),
        Box::new(io::sink()),
        std::path::PathBuf::new(),
        eml_rt::Buffering::Block,
    );
    Ctx::new(
        Arc::new(Control::new(eml_rt::Limits::default())),
        eml_rt::HostRef::new(Arc::new(host)),
    )
}

/// `b"x"` を書く操作を受け付け、token を返す。
#[cfg(test)]
fn submit_x(
    registry: &mut Registry,
    ctx: &mut Ctx<eml_rt::Checked>,
    driver: &mut NoDriver,
    worker: u32,
) -> Word {
    let buf = unsafe { ctx.heap_mut() }.alloc_bytes(b"x");
    let stream = open_stream(ctx, 0, None, Arc::default());
    registry
        .submit_write(ctx, driver, worker, buf, stream)
        .unwrap()
}

/// 取り消しの地点で、もう終わった操作を待つと `Cancelled` で再開し、記録に取り消しの印を付ける。印がないと、取り出しの
/// 行が終わった操作の誤りを投げ、包みが `Cancelled` を投げられない (docs/spec/vm.md の「命令」の `park_io`)。
#[test]
fn a_cancelled_park_on_a_finished_op_marks_the_record_cancelling() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let mut registry = Registry::new(Vec::new());
    let scope = registry.open_root(None, None).unwrap();
    let me = running_task(&mut registry, scope);
    let token = submit_x(&mut registry, &mut ctx, &mut driver, 0);
    let op = registry.token_op(&ctx, token).unwrap();
    registry.ops.clear();
    registry.op_mut(op).unwrap().phase =
        OpPhase::Done(Err(io::Error::from(io::ErrorKind::BrokenPipe)), false);
    registry.task_mut(me).unwrap().cancel = true;
    let run = registry
        .park_io(&ctx, &mut driver, token, me, Word::UNIT)
        .unwrap();
    assert!(matches!(
        run,
        Some(Runnable::ResumeIo { cell, wake: CANCELLED, .. }) if cell == me
    ));
    // 終わった操作の取り消しは driver に頼まない
    assert!(driver.cancelled.is_empty());
    let taken = registry.take_op(&mut ctx, token, 0).unwrap();
    assert!(taken.cancelling && taken.result.is_err());
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    assert!(ctx.heap().leaks().is_empty(), "{:?}", ctx.heap().leaks());
}

/// run の終わりの `sweep_all` は、待つタスクが残した記録の buffer、ストリーム、token をタスクの ID の下で落としてから、
/// タスクのセルが ID を退かせる。記録を後に回すと、退かせるときに記録の物体が残って `TransferLeak` になる
/// (docs/spec/vm.md の「終わり」)。
#[test]
fn sweep_all_frees_a_waited_op_before_its_task() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let mut registry = Registry::new(Vec::new());
    let scope = registry.open_root(None, None).unwrap();
    let me = running_task(&mut registry, scope);
    let worker = unsafe { ctx.heap_mut() }.fork_worker();
    registry.task_mut(me).unwrap().worker = worker;
    with_worker(&mut ctx, worker, 0, |ctx| {
        let token = submit_x(&mut registry, ctx, &mut driver, worker);
        let run = registry
            .park_io(ctx, &mut driver, token, me, Word::UNIT)
            .unwrap();
        assert!(run.is_none());
    });
    registry.sweep_all(&mut ctx, 0).unwrap();
    assert!(registry.ops.is_empty());
    assert!(ctx.heap().leaks().is_empty(), "{:?}", ctx.heap().leaks());
}

/// `bytes` の読みを受け付けて待ち、端が読めるようになった完了を当てる。返すのは token と記録である。
#[cfg(test)]
fn probed_read(
    registry: &mut Registry,
    ctx: &mut Ctx<eml_rt::Checked>,
    driver: &mut NoDriver,
    me: CellRef,
    buf: Word,
) -> (Word, CellRef) {
    let stream = open_stream(ctx, 0, None, Arc::default());
    let token = registry.submit_read(ctx, driver, 0, buf, stream).unwrap();
    let op = registry.token_op(ctx, token).unwrap();
    assert!(
        registry
            .park_io(ctx, driver, token, me, Word::UNIT)
            .unwrap()
            .is_none()
    );
    let key = registry.op_mut(op).unwrap().key;
    driver.done.push(Completion {
        key,
        result: Ok(Done::Size(0)),
        cancelled: false,
    });
    let mut out = Vec::new();
    registry.complete(ctx, driver, 0, &mut out).unwrap();
    // 読めるようになっただけでは再開せず、貸した記憶への読みを新しい鍵で待つ
    assert!(out.is_empty());
    assert!(matches!(
        registry.op_mut(op).unwrap().phase,
        OpPhase::Transfer
    ));
    (token, op)
}

/// 中身 `ab` を持ち、長さの後ろに `room` バイト以上の空きがある一意な `Bytes`。
#[cfg(test)]
fn bytes_ab(ctx: &mut Ctx<eml_rt::Checked>, room: usize) -> Word {
    let heap = unsafe { ctx.heap_mut() };
    let w = heap.alloc_bytes_room(room + 2);
    unsafe { heap.bytes_extend(w, b"ab") }.unwrap()
}

/// 貸した空きが `READ_ROOM` 以上あれば、同じ物体をそのまま貸す。足りなければ、中身を写した新しい物体を貸し、元を
/// 手放す。どちらも貸すのはちょうど `READ_ROOM` バイトである (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_read_lends_a_roomy_buffer_in_place_and_grows_a_short_one() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let mut registry = Registry::new(Vec::new());
    let scope = registry.open_root(None, None).unwrap();
    for (room, in_place) in [(READ_ROOM, true), (0, false)] {
        let me = running_task(&mut registry, scope);
        let buf = bytes_ab(&mut ctx, room);
        let (token, op) = probed_read(&mut registry, &mut ctx, &mut driver, me, buf);
        let lent = registry.op_mut(op).unwrap().buf.unwrap();
        assert_eq!(lent == buf, in_place);
        assert_eq!(driver.lent.pop().unwrap().cap, READ_ROOM);
        let key = registry.op_mut(op).unwrap().key;
        driver.done.push(Completion {
            key,
            result: Ok(Done::Size(0)),
            cancelled: false,
        });
        let mut out = Vec::new();
        registry
            .complete(&mut ctx, &mut driver, 0, &mut out)
            .unwrap();
        let taken = registry.take_op(&mut ctx, token, 0).unwrap();
        let back = taken.buf.unwrap();
        assert_eq!(back, lent);
        assert_eq!(unsafe { &*ctx.heap().bytes(back).unwrap() }, b"ab");
        unsafe { ctx.heap_mut().decref(back) }.unwrap();
    }
    assert!(ctx.heap().leaks().is_empty(), "{:?}", ctx.heap().leaks());
}

/// 貸した記憶へ読んでいる間に取り消された読みも、外が書いたバイトを確定し、それを持つ buffer を返す
/// (docs/spec/stdlib.md の「`Std.IoTest`」)。
#[test]
fn a_read_cancelled_during_its_transfer_commits_the_bytes_it_got() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let mut registry = Registry::new(Vec::new());
    let scope = registry.open_root(None, None).unwrap();
    let me = running_task(&mut registry, scope);
    let buf = bytes_ab(&mut ctx, 0);
    let (token, op) = probed_read(&mut registry, &mut ctx, &mut driver, me, buf);
    let raw = driver.lent.pop().unwrap();
    // 外の書き手として、貸した空きの先頭に書く
    unsafe { std::ptr::copy_nonoverlapping(b"xy".as_ptr(), raw.ptr, 2) };
    registry.task_mut(me).unwrap().cancel = true;
    let mut out = Vec::new();
    registry.unpark(&mut driver, me, &mut out).unwrap();
    let key = registry.op_mut(op).unwrap().key;
    assert_eq!(driver.cancelled, [key]);
    // 取り消しが間に合わず、読みは2バイトで終わった
    driver.done.push(Completion {
        key,
        result: Ok(Done::Size(2)),
        cancelled: false,
    });
    registry
        .complete(&mut ctx, &mut driver, 0, &mut out)
        .unwrap();
    assert!(matches!(
        out.as_slice(),
        [Runnable::ResumeIo { cell, wake: CANCELLED, .. }] if *cell == me
    ));
    let taken = registry.take_op(&mut ctx, token, 0).unwrap();
    assert!(taken.cancelling && matches!(taken.result, Ok(2)));
    let back = taken.buf.unwrap();
    assert_eq!(unsafe { &*ctx.heap().bytes(back).unwrap() }, b"abxy");
    unsafe { ctx.heap_mut().decref(back) }.unwrap();
    assert!(ctx.heap().leaks().is_empty(), "{:?}", ctx.heap().leaks());
}

/// 操作の待ちの期限は満ちても再開せずに driver に取り消しを頼み、記録に期限の印を付ける。取り消した操作の完了が
/// `Woke` で再開し、誤りの結果は期限切れとして読める (docs/spec/vm.md の「時間の待ち」)。
#[test]
fn an_expired_op_wait_cancels_the_op_and_resumes_on_its_completion() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let mut registry = Registry::new(Vec::new());
    let scope = registry.open_root(None, None).unwrap();
    let me = running_task(&mut registry, scope);
    let token = submit_x(&mut registry, &mut ctx, &mut driver, 0);
    let key = driver.next_key;
    let run = registry
        .park_io_until(&ctx, &mut driver, 3, 50, token, me, Word::UNIT)
        .unwrap();
    assert!(run.is_none());
    assert_eq!(
        (registry.next_deadline(), registry.stats.timers),
        (Some(53), 1)
    );
    let mut out = Vec::new();
    registry.expire(53, &mut driver, &mut out).unwrap();
    assert!(out.is_empty());
    assert_eq!(driver.cancelled, [key]);
    assert_eq!(registry.next_deadline(), None);
    driver.done.push(Completion {
        key,
        result: Err(io::Error::other("cancelled")),
        cancelled: true,
    });
    registry
        .complete(&mut ctx, &mut driver, 0, &mut out)
        .unwrap();
    let Some(Runnable::ResumeIo {
        cell,
        wake: WOKE,
        op,
        ..
    }) = out.pop()
    else {
        panic!("the op wait resumes with `Woke`");
    };
    assert_eq!(cell, me);
    assert!(registry.op_timed_out(&ctx, op).unwrap());
    let taken = registry.take_op(&mut ctx, op, 0).unwrap();
    assert!(taken.timed_out && !taken.cancelling);
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    assert!(ctx.heap().leaks().is_empty(), "{:?}", ctx.heap().leaks());
}

/// 期限の付いた操作の待ちは、取り消しで外れても完了で終わっても輪の項を外す。項を残すと、満期に `expire` が待たない
/// タスクを名指す。
#[test]
fn cancelling_or_finishing_a_timed_op_wait_removes_its_timer() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let mut registry = Registry::new(Vec::new());
    let scope = registry.open_root(None, None).unwrap();
    let first = running_task(&mut registry, scope);
    let second = running_task(&mut registry, scope);
    let t1 = submit_x(&mut registry, &mut ctx, &mut driver, 0);
    let k1 = driver.next_key;
    let t2 = submit_x(&mut registry, &mut ctx, &mut driver, 0);
    let k2 = driver.next_key;
    registry
        .park_io_until(&ctx, &mut driver, 0, 50, t1, first, Word::UNIT)
        .unwrap();
    registry
        .park_io_until(&ctx, &mut driver, 0, 60, t2, second, Word::UNIT)
        .unwrap();
    registry.task_mut(first).unwrap().cancel = true;
    let mut out = Vec::new();
    registry.unpark(&mut driver, first, &mut out).unwrap();
    assert!(out.is_empty());
    assert_eq!(driver.cancelled, [k1]);
    assert_eq!(registry.next_deadline(), Some(60));
    driver.done.push(Completion {
        key: k2,
        result: Ok(Done::Size(1)),
        cancelled: false,
    });
    driver.done.push(Completion {
        key: k1,
        result: Err(io::Error::other("cancelled")),
        cancelled: true,
    });
    registry
        .complete(&mut ctx, &mut driver, 0, &mut out)
        .unwrap();
    assert_eq!(registry.next_deadline(), None);
    let resumed: Vec<(CellRef, i64)> = out
        .iter()
        .map(|run| match run {
            Runnable::ResumeIo { cell, wake, .. } => (*cell, *wake),
            _ => panic!("expected an op resume"),
        })
        .collect();
    assert_eq!(resumed, [(second, WOKE), (first, CANCELLED)]);
    for run in out {
        let Runnable::ResumeIo { op, .. } = run else {
            unreachable!("checked above");
        };
        let taken = registry.take_op(&mut ctx, op, 0).unwrap();
        assert!(!taken.timed_out);
        unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    }
    assert!(ctx.heap().leaks().is_empty(), "{:?}", ctx.heap().leaks());
}

/// 0 以下の期限は過ぎた期限なので、輪に載せずにすぐ driver に取り消しを頼み、記録に期限の印を付ける。
#[test]
fn a_past_deadline_cancels_the_op_without_a_timer() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let mut registry = Registry::new(Vec::new());
    let scope = registry.open_root(None, None).unwrap();
    let me = running_task(&mut registry, scope);
    let token = submit_x(&mut registry, &mut ctx, &mut driver, 0);
    let key = driver.next_key;
    let run = registry
        .park_io_until(&ctx, &mut driver, 0, 0, token, me, Word::UNIT)
        .unwrap();
    assert!(run.is_none());
    assert_eq!(driver.cancelled, [key]);
    assert_eq!((registry.next_deadline(), registry.stats.timers), (None, 0));
    driver.done.push(Completion {
        key,
        result: Err(io::Error::other("cancelled")),
        cancelled: true,
    });
    let mut out = Vec::new();
    registry
        .complete(&mut ctx, &mut driver, 0, &mut out)
        .unwrap();
    let Some(Runnable::ResumeIo { wake: WOKE, op, .. }) = out.pop() else {
        panic!("the op wait resumes with `Woke`");
    };
    assert!(registry.op_timed_out(&ctx, op).unwrap());
    let taken = registry.take_op(&mut ctx, op, 0).unwrap();
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    assert!(ctx.heap().leaks().is_empty(), "{:?}", ctx.heap().leaks());
}

/// 正の期限は、操作がもう終わっていて輪に載せないときも `timers` に数える。終わったかどうかはスケジュールで変わるので、
/// 数はプログラムだけで決まる (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn a_deadline_on_a_finished_op_counts_a_timer_without_one_on_the_wheel() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let mut registry = Registry::new(Vec::new());
    let scope = registry.open_root(None, None).unwrap();
    let me = running_task(&mut registry, scope);
    let token = submit_x(&mut registry, &mut ctx, &mut driver, 0);
    let key = driver.next_key;
    driver.done.push(Completion {
        key,
        result: Ok(Done::Size(1)),
        cancelled: false,
    });
    let mut out = Vec::new();
    registry
        .complete(&mut ctx, &mut driver, 0, &mut out)
        .unwrap();
    assert!(out.is_empty());
    let run = registry
        .park_io_until(&ctx, &mut driver, 0, 50, token, me, Word::UNIT)
        .unwrap();
    assert!(matches!(
        run,
        Some(Runnable::ResumeIo { cell, wake: WOKE, .. }) if cell == me
    ));
    assert_eq!((registry.next_deadline(), registry.stats.timers), (None, 1));
    assert!(driver.cancelled.is_empty());
    assert!(!registry.op_timed_out(&ctx, token).unwrap());
    let taken = registry.take_op(&mut ctx, token, 0).unwrap();
    assert!(matches!(taken.result, Ok(1)) && !taken.timed_out);
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    assert!(ctx.heap().leaks().is_empty(), "{:?}", ctx.heap().leaks());
}

/// 受けの記録を期限付きで待ち、期限を満たしてから `result` の完了を当てる。返すのは `op_outcome` の番号である。
/// どちらの番号も `finish` は投げず、包みが種類を決める。
#[cfg(test)]
fn expired_accept_outcome(result: io::Error, cancelled: bool) -> i64 {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let mut registry = Registry::new(Vec::new());
    let scope = registry.open_root(None, None).unwrap();
    let me = running_task(&mut registry, scope);
    let listener = NetListener {
        handle: 0,
        port: 1,
        bin: Arc::default(),
    };
    let listener = unsafe { ctx.heap_mut() }.alloc_foreign(listener, true);
    let token = registry
        .submit_accept(&mut ctx, &mut driver, 0, listener)
        .unwrap();
    let key = driver.next_key;
    let run = registry
        .park_io_until(&ctx, &mut driver, 0, 50, token, me, Word::UNIT)
        .unwrap();
    assert!(run.is_none());
    let mut out = Vec::new();
    registry.expire(50, &mut driver, &mut out).unwrap();
    assert_eq!(driver.cancelled, [key]);
    driver.done.push(Completion {
        key,
        result: Err(result),
        cancelled,
    });
    registry
        .complete(&mut ctx, &mut driver, 0, &mut out)
        .unwrap();
    let Some(Runnable::ResumeIo { wake: WOKE, op, .. }) = out.pop() else {
        panic!("the op wait resumes with `Woke`");
    };
    let code = registry.op_outcome(&ctx, op).unwrap();
    let back = registry.op_stream(&ctx, op).unwrap();
    let taken = registry.take_op(&mut ctx, op, 0).unwrap();
    assert!(taken.timed_out && taken.made.is_none() && taken.failure().is_none());
    unsafe { ctx.heap_mut().decref(back) }.unwrap();
    assert!(ctx.heap().leaks().is_empty(), "{:?}", ctx.heap().leaks());
    code
}

/// 期限が来た後に届いた本物の誤りは、期限に負けたとせず、その種類で報告する。誤りはそれを報告した syscall が消費した
/// ので、後の操作はもう見られない (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_deadline_that_races_a_reset_reports_the_reset() {
    let reset = io::Error::from(io::ErrorKind::ConnectionReset);
    assert_eq!(expired_accept_outcome(reset, false), OUTCOME_RESET);
}

/// 期限の取り消しだけで終わった完了は、期限に負けた番号になる (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_deadline_cancel_ends_late() {
    let cancelled = io::Error::other("cancelled");
    assert_eq!(expired_accept_outcome(cancelled, true), OUTCOME_LATE);
}
