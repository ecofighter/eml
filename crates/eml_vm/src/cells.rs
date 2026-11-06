//! `Async` のタスクとスコープと channel の意味の核 (docs/spec/vm.md の「`Async` のタスク」、「channel」)。VM の
//! `Task`、`Scheduler`、フレームを知らず、走らせる仕事を `Runnable` と不透明な `WaitToken` で返す。ネイティブのコードが
//! 同じ核を `extern "C"` のエントリから呼べるように、`std` と `eml_rt` だけに依存する。
//!
//! 登録表 `Registry` は run に1つで、`Shared` の lock の下にあり、run のすべての `Scheduler` が同じものを持つ。lock の
//! 下では `Ctx` も `Heap` も driver も呼ばない。例外は `start` の `fork_worker` と `share`、`settle_op` と `advance` が
//! `with_worker` の下で行う確保である。捨てる語と退かせる ID は `Discards`、driver に出す操作と取り消しは `IoWork` に
//! 並べ、lock を放してから並べた順に行う。channel の預かり先の操作は、実行系が lock の外で行う
//! (docs/spec/vm.md の「セルと登録表」)。
//!
//! セルが持つ語は数えた参照で、VM が1回入れて1回出す。`Registry` はヒープに触れる `Drop` を持たないので、誤りで終わった
//! run と捨てた run では、触れずに捨てる。
//!
//! 不変条件: スコープの `members` と `nested` の項は、いつも生きているセルを指す。`waiters` の項は古くてもよく、
//! `wake_waiter` が `wait` で確かめる。`k.is_some()` と、`wait` が `Some(Cell(_))`、`Some(Time(_))`、`Some(Io(_))`、
//! `Some(Signal(_))`、`Some(Job(_))` のどれかであることは同値である (`Ready` の継続は列に積んだ仕事が持つ)。`timers` の項
//! `key → Due::Task(t)` があるのは、`t` の `wait` が `Some(Time(key))` のときか、`Some(Io(op))` で記録 `op` の
//! `timer` が `Some(key)` のときだけで、時間の待ちの登録と記録が覚えた鍵には、いつも項がある。タスクがシグナルの
//! 待ち手の列にあるのは、止まっていて `wait == Some(Signal(その番号))` のときだけで、1回だけ現れる。タスクの今のスコープ `current` から親へたどる列は、そのタスクが開いたスコープだけからなり、加わったスコープ `scope` で終わる。入れ子の `run` の根の
//! `opener` は呼んだタスクで、根は誰の今のスコープにもならず、放されるまで呼んだタスクの `run` が名指す。`closed` の
//! スコープは `closing` で、`members` が空である。スコープのセルを放すのは、閉じた後の `release_scope`
//! (`Take::Run` は根の待ちが `closed` で起きた後に、`leave_scope` は今のスコープを親に戻した後に呼ぶ) と、run の終わりの
//! `sweep_all` だけである。
//! タスクのセルを放すのは、ハンドルを持つ側の `take`、自分のハンドルを落とした項の処理、`sweep_all` の3つだけで、
//! 待つ側はハンドルを持つので、待つ相手のセルはその `take` の前に放されない。
//!
//! channel の不変条件: タスクが `senders` か `receivers` にあるのは、そのタスクが止まっていて
//! `wait == Some(Cell(その channel))` のときだけで、1回だけ現れる。`closed` の後に列に待ち手がいるのは、送りの予約
//! `sends` が残っていて受け手が待つときだけである。予約 (`sends`、`recvs`) の残る channel は放さず、channel のセルを
//! 放すのは `settle` と `sweep_all` だけである。品物があるのに受け手が待っている状態は、受け手を起こす
//! 仕事が実行待ちにあるときにだけありうる。積めるのに送り手が待っている状態も同じである。起こす側は枠も品物も取って
//! おかないので、起こされたタスクが速い道をやり直す (docs/spec/vm.md の「channel」)。
//!
//! 操作の記録の不変条件: driver の鍵は記録の番号 (`CellRef`) で、記録は受け付けから取り出しまで1つの番号を持つ。
//! 記録の状態は、操作のない `Idle`、`pending_op` に操作を置いた `Queued`、host が driver に出した `InDriver`
//! (`submitted`)、結果を `arrival` に置いた `Arrived` で、持ち主 (`owner`) が結果を当てて段階を進める。host は操作を
//! driver に出し入れするだけで語に触れず、持ち主がほかの worker なら `Submit`、`CancelIo`、`Completion` の知らせで
//! 受け渡す。`Arrived` の記録は、持ち主が `Completion` を受けるか、次に待つか続けるときに当てる。区画 `k` の `in_flight` は、
//! worker `k` が host で `InDriver` にある記録の数である。記録の `waiter` が `t` を指して完了が
//! まだなら、`t` の `wait` は `Some(Io(その記録))` である。1つの token に記録は1つで、1つのタスクの待ちの登録は1つである。
//! 記録を放すのは、受け付けたタスクの `take_op` と `sweep_all` だけである。
//!
//! 仕事のセルの不変条件: 仕事の `waiter` が `t` を指すのは、仕事がまだ終わっていないときだけで、`t` は仕事を出した
//! `submitter` であり、`t` の `wait` は `Some(Job(その仕事))` である。仕事を待てるのは出したタスクだけである。
//! 仕事のセルを放すのは、`take_job` と `drop_job` の行が呼ぶ `take_job` と、run の終わりの `sweep_all` だけである。
//! park したタスクを途中で捨てる仕組みは、捨てるタスクを `submitter` に持つ仕事のセルを、`with_worker(仕事の ID)` と
//! `retire_worker` の下で落とさなければならない (docs/spec/vm.md の「セルと登録表」)。
//!
//! 取り消しは、印を付ける `mark` と、持ち主が mail か受け箱の `Cancel` の知らせを処理して待ちを外す `unpark` の2段で行う。`CANCELLED` で再開する
//! 仕事をほかのタスクのために作るのは、`unpark` と、取り消しを頼んだ操作の結果を当てる `settle_op` だけである。外からの印付けは `shield` の境目 `boundary` で止まる。
//! `cancel`、`flag`、`cancel_new` はどのタスクの操作からも書く。`current` と `run` は持ち主だけが書き、印を付ける
//! 操作が読む。スコープの `shield` と `boundary` は開くときに1回書き、後は読むだけである。タスクのセルの残りは、
//! そのタスクを動かすか取り消しを処理する持ち主だけが読み書きする (docs/spec/vm.md の「取り消しの届け方」)。

use std::any::Any;
use std::borrow::Cow;
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::ThreadId;

use eml_rt::{
    Completion, Control, Ctx, Done, Fault, HandleBin, HeapError, IoDriver, IoHandle, Mode, Op,
    OpKey, OpenMode, Process, RawBuf, RawResource, Submit, Word,
};

/// 再開するタスクに渡す `Std.Async.Wake` の `Woke` のタグ (docs/spec/stdlib.md の「`Std.Async`」)。
pub(crate) const WOKE: i64 = 0;
/// `Cancelled` のタグ。
pub(crate) const CANCELLED: i64 = 1;
/// `IOErrorKind` の `Cancelled` のタグ。この種類で終わったタスクは失敗に数えない。
pub(crate) const IO_CANCELLED: i64 = 5;
/// 待たずに済む速い道の予算 (docs/spec/stdlib.md の「操作の予算」)。
const BUDGET: u32 = 128;
/// 書きの待ちが予算を1つ余分に使うバイト数。64 KiB の書き1回で予算を使い切り、同じ worker の読み手を起こす
/// (docs/spec/stdlib.md の「操作の予算」)。
const WRITE_UNIT: usize = 512;
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
    pub(crate) fn encode(self) -> i64 {
        (i64::from(self.generation) << 32) | i64::from(self.index)
    }

    pub(crate) fn decode(c: i64) -> CellRef {
        CellRef {
            index: c as u32,
            generation: (c >> 32) as u32,
        }
    }
}

/// token で待つものの番号。実行系が自分の単位 (VM では `TaskId`) に写す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct WaitToken(pub u64);

/// 核が実行系に渡す仕事。`worker` はどの腕でもタスクの論理的な ID で、セルを触る区間で読んでおく。実行系が仕事を積むときに
/// 登録表を読み直さないためである。
pub(crate) enum Runnable {
    /// タスク `cell` を始める。`closure` はエントリ `start_task` に渡す閉包で、仕事が参照を1つ持つ。
    Start {
        cell: CellRef,
        worker: u32,
        closure: Word,
    },
    /// 待っていたタスク `cell` を、継続 `k` に `wake` を渡して再開する。`k` は仕事が参照を1つ持つ。
    Resume {
        cell: CellRef,
        worker: u32,
        k: Word,
        wake: i64,
    },
    /// driver の操作を待っていたタスク `cell` を、継続 `k` に `wake` と token `op` を渡して再開する。`k` は仕事が参照を
    /// 1つ持ち、`op` は記録の番号の即値である。
    ResumeIo {
        cell: CellRef,
        worker: u32,
        k: Word,
        wake: i64,
        op: i64,
    },
    /// token で待つものを起こす。
    Token(WaitToken),
}

/// セルが終わるか閉じるのを待つもの。
enum Waiter {
    /// 継続だけになって待つタスク。継続は待つタスクのセルが持つ。
    Task(CellRef),
    /// スタックを持ったまま待つもの。`Async.run` を呼んだ VM の `Task` で、`owner` はその `Task` を持つ worker である。
    Token { token: WaitToken, owner: u32 },
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
    /// シグナルを待つ。値はシグナルの番号 (0 が `Interrupt`、1 が `Terminate`) で、タスクはその番号の待ち手の列にいる。
    Signal(usize),
    /// 出した仕事が終わるのを待つ。仕事のセルの `waiter` がこのタスクを指す。
    Job(CellRef),
}

/// 時間の輪の項の値。語を持たない (docs/spec/vm.md の「時間の待ち」)。
enum Due {
    /// 時間か期限付きの操作を待つタスクのセル。
    Task(CellRef),
    /// 予定した仮想のシグナルのビット (`Control::INTERRUPT` か `Control::TERMINATE`)。
    Signal(u32),
}

/// シグナルの番号ごとの `Control` のビット。番号は `Std.Async.signal_wait` に渡す値である。
const SIGNAL_BITS: [u32; 2] = [Control::INTERRUPT, Control::TERMINATE];

/// 時間の輪の鍵。期限はミリ秒で、同じ期限は `seq` で登録の順に並ぶ (docs/spec/vm.md の「時間の待ち」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TimerKey {
    deadline: u64,
    seq: u64,
}

struct TaskCell {
    /// タスクを動かす物理的な worker。`start` と救済が登録表の lock の下で書く (docs/spec/vm.md の「セルと登録表」
    /// 「救済」)。
    owner: u32,
    /// このタスクのエントリの `Task` のうち、まだ `Finish` していないものの数。0 の間だけ救済で動かせる
    /// (docs/spec/vm.md の「救済」)。
    entries: u32,
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
    /// 速い道の残りの予算。始めるときと、操作の待ちでない再開のときと、予算を使い切った操作の待ちからの再開のときに
    /// 戻す (docs/spec/stdlib.md の「操作の予算」)。
    budget: u32,
    /// 終わった操作の速い道が予算を使い切って、続く `reschedule` で譲るところである。その再開は取り消しを数えない。
    fast_yield: bool,
    /// 仕事の終わりがこのタスクを起こすか、終わった仕事への park がすぐ再開の項 (`Runnable::Resume`) を返した。`resumed` だけが取り、
    /// 実行待ちからの再開と同じく取り消しの地点の規則で `Cancelled` に替えるかを決める (docs/spec/vm.md の
    /// 「取り消しの届け方」)。
    woke_from_job: bool,
    /// 入れ子の `run` で待っている根。
    run: Option<CellRef>,
    detached: bool,
    /// タスクの上限の旗。共有で、lock の下で誰でも立て、`ResourceLimit` を起こした実行系が `clear_limit` で下ろす
    /// (docs/spec/vm.md の「タスクの上限」)。
    limit: bool,
    /// 待ちをまたぐタスクの数 (取ったページのバイト数)。持ち主だけが書き、`leave` が終わったエントリの分を足し、
    /// `clear_limit` が 0 に戻す (docs/spec/vm.md の「セルと登録表」)。
    bytes: usize,
}

/// エントリの `Task` が動き始めるときにセルから受け取る、タスクの上限の印 (docs/spec/vm.md の「タスクの上限」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskMarks {
    /// 旗が立っている。実行系はエントリの `Task` を保留にする。
    pub(crate) limit: bool,
    /// 前のエントリまでのタスクの数。エントリの `Task` はここから数え始める。
    pub(crate) bytes: usize,
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
    Job(JobCell),
}

/// `compute` の仕事のセル (docs/spec/vm.md の「セルと登録表」)。受け箱の番号を持たないので、仕事を動かす worker が
/// 替わっても、終わりは `submitter` の今の持ち主に route される。
struct JobCell {
    /// 仕事の論理的な ID。終わりの語はこの ID の物体で、取り出す側が `transfer_back` してから退かせる。
    worker: u32,
    /// 仕事を出したタスク。待てるのも取り出せるのもこのタスクだけである。
    submitter: CellRef,
    end: Option<JobEnd>,
    /// 終わりを待つタスク。いれば `submitter` で、その `wait` は `Some(Job(このセル))` である。
    waiter: Option<CellRef>,
}

/// 仕事の終わり方。仕事の実行時エラーはその場で run を止めるので、誤りの変種はない
/// (docs/spec/vm.md の「`compute` の仕事」)。
pub(crate) enum JobEnd {
    /// 仕事の結果。仕事の ID の物体で、セルが参照を1つ持つ。
    Value(Word),
    /// never だけのエフェクトの中断。`clause` は `reperform$` の閉包で、セルが参照を1つ持つ。
    Abort {
        effect: u16,
        depth: u32,
        clause: Word,
    },
}

impl JobEnd {
    /// 終わりが持つ語。
    pub(crate) fn word(&self) -> Word {
        match *self {
            JobEnd::Value(w) | JobEnd::Abort { clause: w, .. } => w,
        }
    }
}

/// driver に出した操作の記録 (docs/spec/vm.md の「操作の記録」)。driver は語を見ないので、操作に貸した記憶の物体と
/// ストリームは記録が持つ。語は `Depot` で公開せず、受け付けたタスクの ID `worker` の物体のまま持つ。
struct OpRecord {
    /// 受け付けたタスクの持ち主の worker。段階を進めて結果を当てるのはこの worker だけである。
    owner: u32,
    /// driver を持ち、操作を driver に出し入れする worker。語には触れない。
    host: u32,
    /// host が driver に出して、まだ結果を `arrival` に置いていない。`cancelling` と同じく登録表の lock の区間の中でだけ
    /// 読み書きし、host は立てる区間で取り消しの印を読む。取り消す側と受け付ける側のどちらかが必ず相手の印を見る
    /// (docs/spec/vm.md の「操作の記録」)。
    submitted: bool,
    /// 持ち主が作って、host がまだ driver に出していない操作。
    pending_op: Option<Op>,
    /// host が受け取り、持ち主がまだ当てていない結果と、取り消しだけで終わったか。
    arrival: Option<(io::Result<Done>, bool)>,
    kind: OpKind,
    phase: OpPhase,
    /// 受け付けたタスクの論理的な worker ID。そのタスクの外で記録の語に触れるときは、この ID に切り替える。
    worker: u32,
    /// 操作に貸した `Bytes` の参照で、受け付けから取り出しまで記録が持つ。読みに貸す間はその物体へのただ1つの参照で、
    /// 書きの物体はほかの参照と共有してよい (docs/spec/object-model.md の T10 の行)。
    buf: Option<Word>,
    /// 受け付けに渡されたストリームかリスナー (`accept`)。`op_stream` が取り戻すまで持つ。
    stream: Option<Word>,
    /// 完了の中身から集める地点で作った `NetListener` か `IoStream` の `Foreign`。取り出しの行が受け取るまで持つ。
    made: Option<Word>,
    /// 待っているタスク。
    waiter: Option<CellRef>,
    /// 待ちの期限の輪の鍵。期限が来るか、完了が届くか、取り消しで外れるまで持つ。
    timer: Option<TimerKey>,
    /// 待つタスクが取り消され、driver に取り消しを頼んだ。完了は `Cancelled` で再開する。
    cancelling: bool,
    /// 待ちの期限が来て、driver に取り消しを頼んだ。完了は `Woke` で再開し、誤りの結果は期限切れとして扱う
    /// (docs/spec/stdlib.md の「`Std.Net`」)。
    timed_out: bool,
}

/// 記録の操作の種類。読みの `Transfer` の完了だけが、外の書いた長さを buffer に確定する。
enum OpKind {
    /// `prefix` は受け付けたストリームの、誤りの文言の前に付ける語である。
    Read {
        prefix: Prefix,
    },
    /// `len` はプログラムが渡した buffer の長さで、待ちの予算の重みになる。
    Write {
        len: usize,
        prefix: Prefix,
    },
    /// `prefix` は誤りの文言の前に付ける `cannot listen on {host}:{port}` である。
    Listen {
        prefix: Box<str>,
    },
    Accept,
    /// `prefix` は `cannot connect to {host}:{port}` である。
    Connect {
        prefix: Box<str>,
    },
    /// `read_line` の行の連鎖。`filled` は `Transfer` の間だけ意味を持ち、空きを貸す前のストリームの buffer の長さで
    /// ある。行を見つけて終わった記録は、終わりの改行を含む行を写して `made` に持つ。`not_utf8` は、見つけた行が
    /// UTF-8 でなく、文字列ではなく `Bytes` に写したことである。`take_line` はその失敗を返し、`take_line_bytes` は
    /// その `Bytes` を返す。
    Line {
        filled: usize,
        not_utf8: bool,
        prefix: Prefix,
    },
    /// `Std.Net.shutdown_write` と `set_nodelay`。どちらも受け付けで終わる。
    ShutdownWrite,
    SetNodelay,
    /// `Std.Stream` の `open`、`create`、`append`。Host が開いたファイルを host の driver に取り込み、`connect` と
    /// 同じく完了の中身からストリームを作る。`write` は書き手として開いたことである (docs/spec/vm.md の「操作の記録」)。
    Open {
        path: Arc<str>,
        write: bool,
    },
    /// `Std.Exec.wait` の、子 (`Std.Exec.Child`) の段の1つの終わりの待ち。`program` はその段のプログラムで、誤りの
    /// 文言は `` cannot wait for `program` `` である。記録の持ち主と host は同じ worker である (docs/spec/vm.md の
    /// 「操作の記録」)。
    Wait {
        program: Arc<str>,
    },
    /// プールの仕事。`from`は仕事の出どころで、取り出しが確かめる。`answer`はプールが返した箱で、記録は取り出しまで
    /// 開けずに持つ。記録はヒープの語を持たない (docs/spec/vm.md の「操作の記録」)。
    Job {
        from: JobSource,
        answer: Option<Box<dyn Any + Send>>,
    },
}

/// プールの仕事の出どころ。取り出しは、取り出す行の出どころと比べる。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum JobSource {
    /// `blocking`のCの関数。値は`CodeBase`のCの関数の番号である。
    C(u32),
}

/// 読みと書きの記録が、誤りの文言の前に付ける語の元。受け付けたストリームの `Origin` で決まる。
enum Prefix {
    /// TCP のストリーム。`cannot read the stream` と `cannot write to the stream` である。
    Net,
    /// `Std.Stream` のファイルで、値は開いたパスである。`` cannot read `p` `` と `` cannot write to `p` `` である。
    File(Arc<str>),
    /// `Std.Exec` の子の pipe で、値は端のつながる段のプログラムである。`` cannot read the output of `p` ``、
    /// `` cannot read the errors of `p` `` と `` cannot write to `p` `` である。
    Child(Arc<str>, ChildPipe),
}

impl OpKind {
    /// `finish` が投げる IO の失敗と `op_prefix` の文言の前に付ける語 (docs/spec/stdlib.md の「`Std.Net`」、
    /// docs/spec/stdlib.md の「`Std.Stream`」)。
    fn prefix(&self) -> Cow<'_, str> {
        match self {
            OpKind::Read {
                prefix: Prefix::Net,
            }
            | OpKind::Line {
                prefix: Prefix::Net,
                ..
            } => "cannot read the stream".into(),
            OpKind::Read {
                prefix: Prefix::File(path),
            }
            | OpKind::Line {
                prefix: Prefix::File(path),
                ..
            } => format!("cannot read `{path}`").into(),
            OpKind::Read {
                prefix: Prefix::Child(program, pipe),
            }
            | OpKind::Line {
                prefix: Prefix::Child(program, pipe),
                ..
            } => format!("cannot read {} `{program}`", pipe.what()).into(),
            OpKind::Write {
                prefix: Prefix::Net,
                ..
            } => "cannot write to the stream".into(),
            OpKind::Write {
                prefix: Prefix::File(path) | Prefix::Child(path, _),
                ..
            } => format!("cannot write to `{path}`").into(),
            OpKind::Listen { prefix } | OpKind::Connect { prefix } => (&**prefix).into(),
            OpKind::Accept => "cannot accept a connection".into(),
            OpKind::ShutdownWrite => "cannot shut down the stream".into(),
            OpKind::SetNodelay => "cannot set the no-delay option".into(),
            OpKind::Open { path, .. } => format!("cannot open `{path}`").into(),
            OpKind::Wait { program } => format!("cannot wait for `{program}`").into(),
            OpKind::Job { .. } => "cannot run a job".into(),
        }
    }

    /// 行の連鎖の記録の `filled` と `not_utf8` を書き換える。prefix は受け付けたときのままにする。
    fn set_line(&mut self, at: usize, bad: bool) {
        if let OpKind::Line {
            filled, not_utf8, ..
        } = self
        {
            (*filled, *not_utf8) = (at, bad);
        }
    }

    /// 待ちが使う予算。書きは長さで重くする。長さはプログラムが決めるので、予算の切れる位置はスケジュールに
    /// よらない。読めた量はスケジュールで変わるので、読みは重くしない (docs/spec/stdlib.md の「操作の予算」)。
    fn cost(&self) -> u32 {
        match self {
            OpKind::Write { len, .. } => u32::try_from(len / WRITE_UNIT)
                .unwrap_or(u32::MAX)
                .saturating_add(1),
            _ => 1,
        }
    }
}

enum OpPhase {
    /// 空きの足りない読みの1つ目の操作。端が読めるようになるのを driver が待っている。buffer はまだ貸していない。
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
        outcome(&self.kind, &self.result, self.timed_out, self.cancelled)
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

/// `op_ready` の結果。続ける、待つ、先に譲るの3つで、std の包みが分ける (docs/spec/vm.md の「操作の記録」)。
const FAST_GO: i64 = 0;
const FAST_WAIT: i64 = 1;
pub(crate) const FAST_YIELD: i64 = 2;

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
/// 消費しているので、種類のまま報告する。ストリームの設定の誤りは種類で分けず、どれも `finish` が投げる
/// (docs/spec/stdlib.md の「`Std.Net`」)。
fn outcome(kind: &OpKind, result: &io::Result<usize>, timed_out: bool, cancelled: bool) -> i64 {
    match result {
        Ok(_) => OUTCOME_OK,
        Err(_) if timed_out && cancelled => OUTCOME_LATE,
        Err(_) if matches!(kind, OpKind::ShutdownWrite | OpKind::SetNodelay) => OUTCOME_FAILED,
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
    /// `reserve_send` が 0 を返し、まだ `push_raw` していない送りの数。
    sends: usize,
    /// `reserve_recv` が列から取り、まだ `pop_raw` していない品物の数。
    recvs: usize,
    /// 待っている送り手。項はいつも生きていて、`wait == Some(Wait::Cell(このセル))` である。
    senders: VecDeque<CellRef>,
    /// 待っている受け手。同上。
    receivers: VecDeque<CellRef>,
    /// 品物の預かり先。`new_chan` で開き、セルを放すときに閉じる。
    depot: Depot,
}

impl ChanCell {
    /// 積めるか。待っている受け手1人につき枠が1つ開くので、容量 0 でも受け手が待っていれば積める。予約した送りは
    /// 枠を1つずつ取っておく。
    fn can_send(&self) -> bool {
        self.items.len() + self.sends < self.cap as usize + self.receivers.len()
    }

    /// 受け手にとって、もう品物が来ないか。閉じても、予約した送りは後から積むので、それを待つ。
    fn drained(&self) -> bool {
        self.closed && self.sends == 0
    }

    /// 放してよいか。予約が残る間は、預かる ID への `publish` と `receive` が lock の外で続くので放さない。
    fn released(&self) -> bool {
        self.drained() && self.items.is_empty() && self.recvs == 0
    }
}

/// channel の予約から確定までの間、worker が持つもの。予約と確定の間には安全点も切り替えも捕まえる失敗もないので、
/// worker に1つで足り、ほかのタスクに渡らない (docs/spec/vm.md の「channel」)。
pub(crate) enum Hold {
    /// 送りの予約。`depot` は channel の預かる ID である。
    Send { chan: CellRef, depot: u32 },
    /// 受け取りの予約。`item` は予約で列から取った品物で、ほかに参照を持たない。
    Recv {
        chan: CellRef,
        depot: u32,
        item: Word,
    },
}

impl Hold {
    /// `push_raw` が channel `c` の送りの予約を取り出す。
    pub(crate) fn take_send(
        hold: &mut Option<Hold>,
        c: i64,
    ) -> Result<(CellRef, Depot), CellFault> {
        match hold.take() {
            Some(Hold::Send { chan, depot }) if chan == CellRef::decode(c) => {
                Ok((chan, Depot::of(depot)))
            }
            _ => Err(CellFault::Internal(
                "an item is pushed without a reserved send",
            )),
        }
    }

    /// `pop_raw` が channel `c` の受け取りの予約を取り出す。
    pub(crate) fn take_recv(
        hold: &mut Option<Hold>,
        c: i64,
    ) -> Result<(CellRef, Depot, Word), CellFault> {
        match hold.take() {
            Some(Hold::Recv { chan, depot, item }) if chan == CellRef::decode(c) => {
                Ok((chan, Depot::of(depot), item))
            }
            _ => Err(CellFault::Internal(
                "an item is popped without a reserved receive",
            )),
        }
    }
}

/// mail の項。落ちたハンドルの項はハンドルの `Drop` が、取り消しの項は持ち主が自分のタスクに印を付けた `mark` が、
/// シグナルの項と上限の旗の項は監視のスレッドが積む。
pub(crate) enum MailItem {
    HandleDropped(CellRef),
    /// タスク `cell` の取り消し。`owner` は積んだ worker で、タスクの持ち主である。取り出すのはその worker だけである。
    Cancel {
        cell: CellRef,
        owner: u32,
    },
    /// 監視のスレッドが中継した保留のシグナル (`Control::INTERRUPT` と `Control::TERMINATE` のビット)。
    Signal(u32),
    /// タスク `cell` への上限の旗。監視のスレッドは登録表の lock を取らないので、lock なしで読んだ古い番号のこともある
    /// (docs/spec/vm.md の「タスクの上限」)。
    Limit(CellRef),
}

#[cfg(test)]
type Hook = Box<dyn FnOnce(&Mail) + Send>;

impl MailItem {
    /// worker `me` が取り出してよい項か。取り消しの項は持ち主だけが、ほかの項はどの worker も取り出す。
    fn for_worker(&self, me: u32) -> bool {
        match *self {
            MailItem::Cancel { owner, .. } => owner == me,
            MailItem::HandleDropped(_) | MailItem::Signal(_) | MailItem::Limit(_) => true,
        }
    }
}

/// run に1つの、ハンドルの破棄処理と監視のスレッドが核へ知らせる列。lock は葉で、持つ間に登録表の lock を取らない。
/// どの worker の集める地点も処理するが、取り消しの項はその持ち主だけが取り出す (docs/spec/vm.md の「セルと登録表」)。
#[derive(Default)]
pub(crate) struct Mail {
    items: Mutex<Vec<MailItem>>,
    /// 項がある。集める地点とデッドロックの判定が lock を取らずに1回の読みで確かめるためにある。
    pending: AtomicBool,
    /// デッドロックの判定が登録表を読んだ後、欄を読み直す前に1回だけ呼ぶ。その間に項が積まれる順序をテストで作る。
    #[cfg(test)]
    hook: Mutex<Option<Hook>>,
}

impl Mail {
    pub(crate) fn push(&self, item: MailItem) {
        let mut items = self.items.lock().unwrap_or_else(PoisonError::into_inner);
        items.push(item);
        self.pending.store(true, Ordering::Release);
    }

    /// まだ取り出していない項がある。ほかの worker の取り消しの項だけが残っていても真である。
    pub(crate) fn pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn set_hook(&self, hook: impl FnOnce(&Mail) + Send + 'static) {
        *self.hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(hook));
    }

    #[cfg(test)]
    pub(crate) fn run_hook(&self) {
        let hook = self
            .hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(hook) = hook {
            hook(self);
        }
    }

    /// worker `me` が取り出してよい項を、積んだ順に取り出す。ほかの worker の取り消しの項は順を保って残す。持ち主が
    /// 自分のタスクの取り消しを自分で取り出すので、印を付けてから待ちを外すまでに、持ち主のほかのタスクは動かない
    /// (docs/spec/vm.md の「取り消しの届け方」)。
    pub(crate) fn take(&self, me: u32) -> Vec<MailItem> {
        if !self.pending.load(Ordering::Acquire) {
            return Vec::new();
        }
        let mut items = self.items.lock().unwrap_or_else(PoisonError::into_inner);
        // 1つの worker の上では、どの項もその worker のものなので、列ごと取り出す
        if items.iter().all(|item| item.for_worker(me)) {
            self.pending.store(false, Ordering::Release);
            return std::mem::take(&mut *items);
        }
        let (mine, rest): (Vec<_>, Vec<_>) = items.drain(..).partition(|item| item.for_worker(me));
        *items = rest;
        mine
    }

    /// 残った項をすべて取り出す。run の終わりに捨てるときに使う。
    pub(crate) fn take_all(&self) -> Vec<MailItem> {
        let mut items = self.items.lock().unwrap_or_else(PoisonError::into_inner);
        self.pending.store(false, Ordering::Release);
        std::mem::take(&mut *items)
    }
}

/// run の worker が共有する、登録表とその mail。`run_scheduled` が run ごとに作り、すべての `Scheduler` に配る。
pub(crate) struct Shared {
    pub(crate) cells: Mutex<Registry>,
    pub(crate) mail: Mail,
}

impl Shared {
    pub(crate) fn new(registry: Registry) -> Shared {
        Shared {
            cells: Mutex::new(registry),
            mail: Mail::default(),
        }
    }

    /// 登録表の lock を取る。lock の下で panic した worker は run を致命的な誤りにし、`Machine` も使わなくするので、
    /// 毒が付いても続けてよい。
    pub(crate) fn lock(&self) -> MutexGuard<'_, Registry> {
        self.cells.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// `Task a` の `Foreign` の中身。
pub(crate) struct TaskHandle {
    cell: CellRef,
    shared: Arc<Shared>,
    /// `take` が受け取った。受け取ったハンドルはセルをもう指さないので、落ちても知らせない。
    taken: bool,
}

impl Drop for TaskHandle {
    // 破棄処理はヒープの `decref` の中で走るので、登録表の lock を取らずに mail に積むだけにする。前の run のハンドルは、
    // 自分の終わった run の mail に積むので、生きている登録表には届かない (docs/spec/vm.md の「`Async` のタスク」)
    fn drop(&mut self) {
        if !self.taken {
            self.shared.mail.push(MailItem::HandleDropped(self.cell));
        }
    }
}

/// リスナーとストリームが、どの worker の driver のハンドルかを表す。ハンドルを引く操作はその worker (host) が driver に
/// 出す。`listen` と `connect` の結果は受け付けた worker の driver に縛る。`accept` が `Done::Accepted` で返したソケットは
/// 縛らずに持ち、最初に操作を受け付ける worker の driver に縛る (docs/spec/vm.md の「操作の記録」)。
pub(crate) enum Bind {
    /// worker `host` の driver のハンドル `handle`。落ちたら、その driver の bin `bin` に積む。
    Bound {
        host: u32,
        handle: IoHandle,
        bin: Arc<HostBin>,
    },
    /// まだどの driver にも取り込んでいない、資源 (受けたソケット)。落ちたらその場で閉じる。
    Unbound(RawResource),
    /// 取り込みを driver が断った。資源はそのときに閉じた。
    Lost,
}

impl Bind {
    fn host(&self) -> Option<u32> {
        match self {
            Bind::Bound { host, .. } => Some(*host),
            Bind::Unbound(_) | Bind::Lost => None,
        }
    }

    fn handle(&self) -> Result<IoHandle, CellFault> {
        match self {
            Bind::Bound { handle, .. } => Ok(*handle),
            Bind::Unbound(_) | Bind::Lost => {
                Err(CellFault::Internal("a stream is used before it is bound"))
            }
        }
    }

    /// ヒープの `decref` の中で走るので driver に触れず、driver が次に閉じるハンドルとして積むだけにする
    /// (docs/spec/vm.md の「時計と driver」)。縛っていないソケットは driver の外にあるので、欄と一緒に落ちて閉じる。
    fn close(&self) {
        if let Bind::Bound { handle, bin, .. } = self {
            bin.push(*handle);
        }
    }
}

/// `Std.Net.TcpListener` の `Foreign` の中身。driver のハンドルと、待ち受けるポートを持つ。
pub(crate) struct NetListener {
    bind: Bind,
    port: u16,
}

impl Drop for NetListener {
    fn drop(&mut self) {
        self.bind.close();
    }
}

/// `Std.Net.TcpStream`、`Std.Stream.Reader`、`Std.Stream.Writer` の `Foreign` の中身。driver のハンドルと、何を開いたか
/// と、ストリームの読みの buffer を持つ。buffer は `read_line` が読んでまだ返していないバイトで、`rest[start..]` が残り
/// である。`read_line` で読まないストリームでは空のままで、確保も持たない (docs/spec/stdlib.md の「`Std.Net`」、
/// docs/spec/vm.md の「操作の記録」)。
pub(crate) struct IoStream {
    bind: Bind,
    origin: Origin,
    rest: Vec<u8>,
    start: usize,
}

/// ストリームが何を開いたものか。読みの受け付けの形と、誤りの文言の前に付ける語がこれで決まる。
enum Origin {
    /// TCP のソケット。値は作ったときの相手の番地である。
    Peer(SocketAddr),
    /// `Std.Stream` が開いた通常のファイル。値は開いたときに渡されたままのパスである。
    File(Arc<str>),
    /// `Std.Exec` の子の pipe。`program` は端のつながる段 (標準入力なら先頭、標準出力と標準エラーなら最後) の
    /// プログラムである。
    Child { program: Arc<str>, pipe: ChildPipe },
}

/// `Std.Exec` の子の pipe の向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildPipe {
    /// 先頭の段の標準入力で、親が書く。
    Stdin,
    /// 最後の段の標準出力で、親が読む。
    Stdout,
    /// 最後の段の標準エラーで、親が読む。
    Stderr,
}

impl ChildPipe {
    /// 誤りの文言で、この端を指す語。
    fn what(self) -> &'static str {
        match self {
            ChildPipe::Stdin => "the input of",
            ChildPipe::Stdout => "the output of",
            ChildPipe::Stderr => "the errors of",
        }
    }
}

impl IoStream {
    /// ソケットか。ソケットの読みだけが、空きのない buffer で読めるようになるのを待つ (docs/spec/vm.md の「操作の記録」)。
    fn is_socket(&self) -> bool {
        matches!(self.origin, Origin::Peer(_))
    }

    /// このストリームの読みと書きの記録が、誤りの文言の前に付ける語。
    fn prefix(&self) -> Prefix {
        match &self.origin {
            Origin::Peer(_) => Prefix::Net,
            Origin::File(path) => Prefix::File(Arc::clone(path)),
            Origin::Child { program, pipe } => Prefix::Child(Arc::clone(program), *pipe),
        }
    }

    fn pending(&self) -> &[u8] {
        &self.rest[self.start..]
    }

    /// 残りの先頭の `n` バイトを返したことにする。残りがなくなれば確保を放す。
    fn consume(&mut self, n: usize) {
        self.start += n;
        if self.start == self.rest.len() {
            self.rest = Vec::new();
            self.start = 0;
        }
    }

    /// 残りの `from` バイト目から後に改行があるか、`eof` なら、残りの最初の行の、終わりの改行を含めた長さを返す。`eof`
    /// で改行がなければ、残りのすべてが行である (EOF の前の最後の行)。行の中身 (終わりの `\n` か `\r\n` を除いたバイト、
    /// EOF では残りのすべて) が `LINE_LIMIT` バイトを超えれば `TooLong` である。上限を中身の長さで決めるので、結果は
    /// バイトが読みにどう分かれて届いたかによらない (docs/spec/stdlib.md の「`Std.Net`」)。どちらでもなければ `None` で
    /// ある。どの場合も残りを変えない。行は `take_line_text` が文字列に写して返したことにする。
    fn find_line(&self, from: usize, eof: bool) -> Option<Found> {
        find_line(self.pending(), from, eof)
    }

    /// 改行のまだ来ない行の中身の、今わかっている長さ。
    fn open_line_len(&self) -> usize {
        open_line_len(self.pending())
    }

    /// 返したバイトを詰め、buffer の長さの後ろの空きが `MIN_ROOM` バイト以上あれば、そのうち `READ_ROOM` バイトまでを
    /// 貸す。`grow` なら、空きが足りないときに先に `max(長さ, READ_ROOM)` バイトを足して広げ、`READ_ROOM` バイトを貸す。
    /// 返すのは貸す前の長さと貸した記憶で、空きが足りず広げないなら `None` である (docs/spec/vm.md の「操作の記録」)。
    /// 記憶は `Vec` 自身の確保の中の未初期化の空きで、eml のヒープの外である。`Vec::as_mut_ptr` は buffer への参照を
    /// 作らないので、完了まで `rest` に触れなければ、貸した位置は書かれるまで有効である (docs/spec/vm.md の「時計と
    /// driver」の `RawBuf`)。
    fn lend_room(&mut self, grow: bool) -> Option<(usize, RawBuf)> {
        if self.start > 0 {
            self.rest.drain(..self.start);
            self.start = 0;
        }
        let filled = self.rest.len();
        if self.rest.capacity() - filled < MIN_ROOM {
            if !grow {
                return None;
            }
            self.rest.reserve_exact(filled.max(READ_ROOM));
        }
        let raw = RawBuf {
            ptr: self.rest.as_mut_ptr().wrapping_add(filled),
            cap: (self.rest.capacity() - filled).min(READ_ROOM),
        };
        Some((filled, raw))
    }

    /// 貸した空きのうち、driver が書いた先頭の `n` バイトを長さに確定する。残りがなければ確保を放す。
    ///
    /// # Safety
    /// `filled` は `lend_room` が返した長さで、その後 `rest` に触れていない。`n` は貸した記憶の `cap` 以下で、driver が
    /// その記憶の先頭に書き終えたバイト数である。
    unsafe fn commit_room(&mut self, filled: usize, n: usize) {
        debug_assert!(filled == self.rest.len());
        debug_assert!(filled + n <= self.rest.capacity());
        // `n <= cap` なので `filled + n` は貸した空きの内で、その前のバイトはみな初期化してある
        unsafe { self.rest.set_len(filled + n) };
        if self.rest.is_empty() {
            self.rest = Vec::new();
        }
    }
}

impl Drop for IoStream {
    fn drop(&mut self) {
        self.bind.close();
    }
}

/// `Std.Exec.Child` の `Foreign` の中身。段ごとの子と、まだ取り出していない pipe の親の端を持つ。端は driver に
/// 縛らずに持ち、取り出した `Stream.Writer` か `Stream.Reader` の最初の操作が、その worker の driver に取り込む
/// (docs/spec/vm.md の「時計と driver」)。
pub(crate) struct ExecChild {
    stages: Vec<ExecStage>,
    stdin: Option<Bind>,
    stdout: Option<Bind>,
    stderr: Option<Bind>,
}

/// `ExecChild` の段の1つ。
struct ExecStage {
    program: Arc<str>,
    /// まだ回収していない子。回収した段は `None` で、`code` を持つ。
    process: Option<Box<dyn Process>>,
    code: Option<i64>,
}

impl ExecChild {
    /// `Std.Exec.spawn_raw` が起動した子。`programs` と `processes` は段ごとに先頭から並び、数が同じである。`stdin`、
    /// `stdout`、`stderr` は driver の `pipe` が作った親の端である。
    pub(crate) fn new(
        programs: Vec<Box<str>>,
        processes: Vec<Box<dyn Process>>,
        stdin: Option<RawResource>,
        stdout: Option<RawResource>,
        stderr: Option<RawResource>,
    ) -> ExecChild {
        ExecChild {
            stages: programs
                .into_iter()
                .zip(processes)
                .map(|(program, process)| ExecStage {
                    program: program.into(),
                    process: Some(process),
                    code: None,
                })
                .collect(),
            stdin: stdin.map(Bind::Unbound),
            stdout: stdout.map(Bind::Unbound),
            stderr: stderr.map(Bind::Unbound),
        }
    }

    fn stage(&self, k: usize) -> Result<&ExecStage, CellFault> {
        self.stages.get(k).ok_or(CellFault::Internal(
            "a wait names a stage the child does not have",
        ))
    }

    /// 段 `k` の子が、driver が終わりを待てる OS の子なら、その形。回収した段と、OS の子を持たない Host の段は `None`
    /// である。
    fn raw(&self, k: usize) -> Result<Option<eml_rt::RawProcess>, CellFault> {
        Ok(self.stage(k)?.process.as_ref().and_then(|p| p.raw()))
    }
}

/// 取り出していない端を閉じてから、まだ回収していない段をすべて kill し、先頭から待つ。誤りは捨てる。端を先に閉じる
/// のは、標準入力を読む子に EOF を届け、kill の前に終われるようにするためである (docs/spec/stdlib.md の「`Std.Exec`」)。
impl Drop for ExecChild {
    fn drop(&mut self) {
        for bind in [self.stdin.take(), self.stdout.take(), self.stderr.take()]
            .into_iter()
            .flatten()
        {
            bind.close();
        }
        for stage in &mut self.stages {
            if let Some(process) = &mut stage.process {
                let _ = process.kill();
            }
        }
        for stage in &mut self.stages {
            if let Some(mut process) = stage.process.take() {
                let _ = process.wait();
            }
        }
    }
}

/// host の driver の bin と、そこへ積んだ印。driver が bin を空にするのは自分の待ちと集める地点だけなので、ほかの
/// worker で落としたハンドルは、host を起こさなければ閉じないまま残りうる。そこで `Drop` は葉の lock で bin に積み、
/// host のスレッドでなければ印を立てるだけにする。ほかの worker が集める地点で印を見て host を起こし、host は集める
/// 地点で印を下ろして bin を空にする。host が自分で落としたハンドルも、driver に操作を出していない host の集める地点は
/// 閉じないので、ほかの worker の driver で EOF を待つ相手が止まったままになる。そのため `close_own` の bin では、host
/// も印を立てる。立てるのは各 worker が自分の driver を持つ run だけなので、worker が1つの run の振る舞いは変わらない
/// (docs/spec/vm.md の「時計と driver」)。
pub(crate) struct HostBin {
    pub(crate) bin: Arc<HandleBin>,
    /// host の worker のスレッド。
    host: ThreadId,
    /// host が自分で落としたときも印を立てる。
    close_own: bool,
    /// `CLOSE_NONE`、`CLOSE_DROPPED`、`CLOSE_RUNG`、`CLOSE_OWN` のどれか。
    state: AtomicU8,
}

/// 印がない。
const CLOSE_NONE: u8 = 0;
/// host でない worker が積み、まだ誰も host を起こしていない。
const CLOSE_DROPPED: u8 = 1;
/// ほかの worker が host を起こし、host がまだ印を下ろしていない。起こしを重ねないためにある。
const CLOSE_RUNG: u8 = 2;
/// host が自分で積んだ。host は動いているので、誰も起こさない。
const CLOSE_OWN: u8 = 3;

impl HostBin {
    /// 今のスレッドを host とする。host の worker が run の始めか最初の受け付けで作る。
    pub(crate) fn new(bin: Arc<HandleBin>, close_own: bool) -> HostBin {
        HostBin {
            bin,
            host: std::thread::current().id(),
            close_own,
            state: AtomicU8::new(CLOSE_NONE),
        }
    }

    fn push(&self, handle: IoHandle) {
        self.bin
            .dropped
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(handle);
        // 積んだ後に立てるので、印を見て起こされた host は、bin を空にするときにこのハンドルを見る
        if std::thread::current().id() != self.host {
            self.state.store(CLOSE_DROPPED, Ordering::Release);
        } else if self.close_own {
            // ほかの worker の印は残す。host はどの印でも bin を空にする
            let _ = self.state.compare_exchange(
                CLOSE_NONE,
                CLOSE_OWN,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }

    /// ほかの worker が積んで、まだ host を起こしていなければ、起こす役を取って真を返す。
    pub(crate) fn take_ring(&self) -> bool {
        self.state.load(Ordering::Relaxed) == CLOSE_DROPPED
            && self
                .state
                .compare_exchange(
                    CLOSE_DROPPED,
                    CLOSE_RUNG,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_ok()
    }

    /// host が印を下ろし、印があれば真を返す。真なら host は driver に bin を空にさせる。
    pub(crate) fn take_dropped(&self) -> bool {
        self.state.load(Ordering::Relaxed) != CLOSE_NONE
            && self.state.swap(CLOSE_NONE, Ordering::Acquire) != CLOSE_NONE
    }
}

#[cfg(test)]
impl Default for HostBin {
    fn default() -> HostBin {
        HostBin::new(Arc::default(), false)
    }
}

/// 登録表が数える仕事の回数。検査モードでなくても数えるが、`RunStats` に写すのは検査モードだけである
/// (docs/spec/vm.md の「`RunStats`」)。
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct TaskStats {
    /// `start` が作ったタスクのセルの数。
    pub(crate) tasks: u64,
    /// `park`、`ready`、`park_time`、`park_drain`、`park_send`、`park_recv`、`park_io`、`park_io_until` が渡された
    /// 継続の数と、終わった操作で続けた `op_ready` の数。driver の操作の待ちは `io_wait` と `io_wait_until` の呼び出しの
    /// 数になる。
    pub(crate) parks: u64,
    /// スコープの最初の失敗より後に終わった、失敗のタスクの数。
    pub(crate) dropped_failures: u64,
    /// 取り消しが届いた回数。待つ前に取り消しを見つけた `park`、`park_time`、`park_drain`、`park_send`、`park_recv`、
    /// `park_io`、`park_io_until`、`park_signal`、mail の処理が外した待ちと取り消しを頼んだ操作の待ち、取り消しで再開する `yield_now`
    /// (速い道が譲る `reschedule` は除く)、真を返した `cancel_point`、偽を返した `claim_scope` を数える
    /// (docs/spec/vm.md の「`RunStats`」)。
    pub(crate) cancels: u64,
    /// `ms > 0` で、地点で取り消されなかった時間の待ちの数。`park_time` と `park_io_until` と、終わった操作で続けるか
    /// 譲った `op_ready` を数える。`park_io_until` は、操作がもう終わっていて輪に載せなかったときも数える。
    pub(crate) timers: u64,
    /// channel に品物を積んだ回数と取った回数の和。
    pub(crate) channel_ops: u64,
    /// `park_send` と `park_recv` が渡された継続の数。`parks` にも数える。
    pub(crate) channel_waits: u64,
    /// `sweep_all` が放した channel の数。
    pub(crate) channels_swept: u64,
    /// 決着させた driver の操作の結果の数。受け付けで終わった操作と、driver から取り出した完了の和である。
    pub(crate) completions: u64,
}

/// 登録表の中の、物理的な worker ごとの区画 (docs/spec/vm.md の「セルと登録表」)。
pub(crate) struct Part {
    /// 時間の輪。項は時間を待つタスクのセルの番号か予定したシグナルで、語を持たない (docs/spec/vm.md の「時間の待ち」)。
    /// タスクの項は、そのタスクの持ち主の区画に置く。
    timers: BTreeMap<TimerKey, Due>,
    /// 次に登録するタイマーの `seq`。
    timer_seq: u64,
    /// この worker が host で、driver に出してまだ結果を受け取っていない記録の数。0 なら集める地点は driver に触れない。
    /// driver の `pending` を読まないのは、仮想の呼び出しで、driver によっては lock を伴うためである。
    in_flight: usize,
}

impl Part {
    fn new() -> Part {
        Part {
            timers: BTreeMap::new(),
            timer_seq: 0,
            in_flight: 0,
        }
    }

    /// 期限 `deadline` の項を `due` で輪に載せ、鍵を返す。同じ期限は登録の順に並ぶ。
    fn add_timer(&mut self, deadline: u64, due: Due) -> TimerKey {
        let key = TimerKey {
            deadline,
            seq: self.timer_seq,
        };
        self.timer_seq += 1;
        self.timers.insert(key, due);
        key
    }
}

/// 待ちを外したタスクか token を、持ち主が再開するための起こし。継続は持たず、持ち主が `take_wake` でセルから取り出す
/// (docs/spec/vm.md の「セルと登録表」)。
pub(crate) enum Woken {
    /// タスク `cell` を `wake` で再開する。`op` は操作の待ちからの再開のときの記録である。
    Task {
        cell: CellRef,
        wake: i64,
        op: Option<CellRef>,
    },
    Token {
        owner: u32,
        token: WaitToken,
    },
}

/// `take_wake` の答え。
pub(crate) enum Route {
    /// 持ち主が自分なので、この仕事を走らせる。
    Run(Runnable),
    /// 持ち主はほかの worker で、値はその番号である。起こしはその worker へ回す。
    Forward(u32),
    /// 起こしたセルはもう待っていない。
    Stale,
}

/// ほかの worker の受け箱へ知らせを積む道。登録表の lock の下で呼び、持ち主を読んだのと同じ区間で積む。持ち主が
/// 変わる前に積んだ知らせを、受けた worker が今の持ち主へ回せるようにするためである (docs/spec/vm.md の
/// 「セルと登録表」)。実行系が実装する。このファイルは worker の表を知らないからである。
pub(crate) trait Post {
    /// worker `k` に、持ち主として起こしを受け取らせる。
    fn wake(&self, k: u32, w: Woken);
    /// worker `k` に、持ち主としてタスク `cell` の取り消しを届ける。
    fn cancel(&self, k: u32, cell: CellRef);
    /// worker `k` に、持ち主としてタスク `cell` を始めさせる。`closure` は `start` が論理的な ID `worker` に印を付けた
    /// 閉包で、知らせが参照を1つ持つ。
    fn place(&self, k: u32, cell: CellRef, worker: u32, closure: Word);
    /// worker `k` に、host として記録 `op` に置いた操作を driver に出させる。
    fn submit(&self, k: u32, op: CellRef);
    /// worker `k` に、host として記録 `op` の driver の操作を取り消させる。
    fn cancel_io(&self, k: u32, op: CellRef);
    /// worker `k` に、持ち主として記録 `op` に置いた結果を当てさせる。
    fn completion(&self, k: u32, op: CellRef);
    /// worker `k` に、持ち主として動いているタスク `cell` の上限の旗を届ける。
    fn limit(&self, k: u32, cell: CellRef);
}

/// 登録表を触る worker と、取り消しと起こしの宛先。持ち主が自分なら取り消しは mail に積み、起こしはその場で走らせる。
/// ほかの worker なら `post` で持ち主の受け箱に送る (docs/spec/vm.md の「タスクとスケジュール」)。`host` は、この
/// worker のタスクが新しいハンドルを作る操作を受け付ける worker である。既にあるハンドルを引く操作は、ハンドルを
/// 縛った worker が受け付ける (docs/spec/vm.md の「時計と driver」)。
#[derive(Clone, Copy)]
pub(crate) struct Here<'a> {
    pub(crate) me: u32,
    pub(crate) host: u32,
    pub(crate) mail: &'a Mail,
    pub(crate) post: &'a dyn Post,
}

/// 登録表の lock を放してから並べた順に行う、語の破棄と ID の退かせ (docs/spec/vm.md の「セルと登録表」)。
#[derive(Default)]
pub(crate) struct Discards(Vec<Discard>);

enum Discard {
    /// 語を、それを持つ ID の下で落とす。
    Word(Word, u32),
    /// 預かる ID を退かせる。
    Retire(u32),
}

impl Discards {
    fn word(&mut self, w: Word, worker: u32) {
        self.0.push(Discard::Word(w, worker));
    }

    fn retire(&mut self, worker: u32) {
        self.0.push(Discard::Retire(worker));
    }

    /// 並べた順に行う。ID の物体が残っていれば、退かせるときに `TransferLeak` がヒープの誤りとして出る。
    pub(crate) fn run<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        depth_offset: usize,
    ) -> Result<(), CellFault> {
        for discard in self.0.drain(..) {
            match discard {
                Discard::Word(w, worker) => Depot::of(worker).discard(ctx, w, depth_offset)?,
                Discard::Retire(worker) => Depot::of(worker).close(ctx)?,
            }
        }
        Ok(())
    }
}

/// 登録表の lock を放してから行う driver の呼び出し。受け付けを並べた順に出し、その後で取り消しを頼む。
#[derive(Default)]
pub(crate) struct IoWork {
    /// 出す記録と操作と、取り出す区間で取り消しの印を読んだか。印があれば、driver が `Pending` を返したときだけ、
    /// 出した直後に取り消しを頼む。
    submits: Vec<(CellRef, Op, bool)>,
    cancels: Vec<CellRef>,
}

impl IoWork {
    /// 取り消しだけを頼む。受け付けを並べない区間の後に呼ぶ。
    fn cancel_all(&mut self, driver: &mut dyn IoDriver) {
        debug_assert!(self.submits.is_empty(), "a submit is left unsent");
        for op in self.cancels.drain(..) {
            driver.cancel(op_key(op));
        }
    }

    /// `cancel_all` を、driver を持たないかもしれない worker が呼ぶ。取り消しを積むのは host の worker だけなので、
    /// driver がないのに取り消しがあれば内部の誤りである。
    pub(crate) fn cancel_on(
        &mut self,
        driver: Option<&mut (dyn IoDriver + 'static)>,
    ) -> Result<(), CellFault> {
        match driver {
            Some(driver) => self.cancel_all(driver),
            None if self.cancels.is_empty() && self.submits.is_empty() => {}
            None => {
                return Err(CellFault::Internal(
                    "a worker without a driver cancels an op",
                ));
            }
        }
        Ok(())
    }
}

/// `arrive` の答え。結果を当てるのは記録の持ち主だけである (docs/spec/vm.md の「操作の記録」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arrival {
    /// 持ち主が自分なので、同じ区間で当てる。
    Settle,
    /// 持ち主はほかの worker で、待つタスクがいる。値はその番号で、持ち主へ `Completion` を送る。
    Send(u32),
    /// 持ち主はほかの worker で、まだ待っていない。持ち主が次に待つか続けるかを決めるときに当てる。
    Leave,
}

/// `cancel_io` の答え。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelPath {
    /// まだ driver に出していない。host が出す区間で印を読み、出した後に取り消す。
    Flag,
    /// 自分が host なので、lock を放してから自分の driver に頼む。
    Local,
    /// host の worker に頼む。
    Remote(u32),
}

/// 1回の実行のセルの表。実行ごとに作る (docs/spec/vm.md の「`Async` のタスク」)。
pub(crate) struct Registry {
    /// 添字ごとの generation と中身。空いた添字の generation は、次に出す番号のものである。
    slots: Vec<(u32, Option<CellKind>)>,
    free: Vec<u32>,
    pub(crate) stats: TaskStats,
    /// 持ち主を `start` を実行した worker と違う worker にしたタスクの数。両方のモードで数え、`RunStats` には入れない
    /// (docs/spec/vm.md の「`Async` のタスク」)。
    pub(crate) placed_tasks: u64,
    /// worker ごとの区画。添字が worker の番号である。
    parts: Vec<Part>,
    /// シグナルの番号ごとの、待ち始めた順の待ち手。項はいつも生きていて、`wait == Some(Wait::Signal(その番号))` である。
    signal_waiters: [Vec<CellRef>; 2],
    /// 届けたときに待つタスクのいなかったシグナルのビット。集める地点の後で run を止める
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) unheard: u32,
    /// OS のシグナルを待つタスクへ中継する run か。worker 0 が run の始めに1回だけ書く。見張りの語を書くのは集める
    /// worker のどれでもよいが、中継を決める監視のスレッドは worker 0 だけが持つので、ここに置く
    /// (docs/spec/vm.md の「上限と割り込み」)。
    pub(crate) relay: bool,
    /// driver を持つ worker ごとの、その driver が閉じるハンドルの bin。持ち主が host の driver の結果から作るリスナーと
    /// ストリームは、落ちたときに host の bin に積む (docs/spec/vm.md の「操作の記録」)。
    bins: Vec<Option<Arc<HostBin>>>,
}

impl Registry {
    /// `generations` は前の run までに使った添字の、次に出す generation である (`next_generations`)。前の run の
    /// `Scope` と `Chan a` の値がこの run のセルを指さないように、添字ごとに generation を続ける。`workers` は run の
    /// worker の数で、区画をその数だけ作る。
    pub(crate) fn new(generations: Vec<u32>, workers: usize) -> Registry {
        // 小さい添字から使うように、逆の順に積む
        let free = (0..generations.len())
            .rev()
            .filter(|&index| generations[index] != RETIRED)
            .map(|index| u32::try_from(index).expect("the cells fit in u32 positions"))
            .collect();
        Registry {
            slots: generations.into_iter().map(|g| (g, None)).collect(),
            free,
            stats: TaskStats::default(),
            placed_tasks: 0,
            parts: (0..workers.max(1)).map(|_| Part::new()).collect(),
            signal_waiters: [Vec::new(), Vec::new()],
            unheard: 0,
            relay: false,
            bins: vec![None; workers.max(1)],
        }
    }

    /// worker `k` の driver の bin を覚える。driver を持つ worker が run の始めに1回だけ書く。
    pub(crate) fn set_bin(&mut self, k: usize, bin: Arc<HostBin>) {
        self.bins[k] = Some(bin);
    }

    /// host `k` の bin。driver を持つ worker は run の始めに bin を覚え、run の表の欄にも置く。ほかの worker はその欄の
    /// 印を見て host を起こすので、欄にない bin を作ると、落としたハンドルを閉じさせる起こしが届かない
    /// (docs/spec/vm.md の「操作の記録」)。run の始めを通らない単体テストだけは、ここで driver の bin `raw` から作る。
    fn host_bin(&mut self, k: u32, raw: Arc<HandleBin>) -> Result<Arc<HostBin>, CellFault> {
        let bin = &mut self.bins[k as usize];
        if bin.is_none() && cfg!(test) {
            *bin = Some(Arc::new(HostBin::new(raw, false)));
        }
        bin.clone()
            .ok_or(CellFault::Internal("a host has no bin for its handles"))
    }

    /// 予定した仮想のシグナル (期限のミリ秒とビットの組) を、並びの順に worker 0 の時間の輪へ載せる。run の始めに、
    /// どのタイマーよりも先に呼ぶので、同じ期限ではプログラムのタイマーより先に満ちる。`timers` には数えない
    /// (docs/spec/vm.md の「時間の待ち」)。
    pub(crate) fn schedule_signals(&mut self, signals: impl IntoIterator<Item = (u64, u32)>) {
        for (deadline, bit) in signals {
            self.parts[0].add_timer(deadline, Due::Signal(bit));
        }
    }

    /// worker `k` が host で、driver に出してまだ結果を受け取っていない操作の数。
    pub(crate) fn in_flight(&self, k: usize) -> usize {
        self.parts[k].in_flight
    }

    /// driver に出してまだ結果を受け取っていない操作の、すべての区画の和。driver の約束から、host ごとの数はその
    /// driver の `pending()` に等しい (docs/spec/vm.md の「時計と driver」)。
    pub(crate) fn in_flight_all(&self) -> usize {
        self.parts.iter().map(|part| part.in_flight).sum()
    }

    /// 輪が空でない区画ごとの、持ち主の番号といちばん早い期限。
    pub(crate) fn deadlines(&self) -> impl Iterator<Item = (usize, u64)> + '_ {
        self.parts.iter().enumerate().filter_map(|(owner, part)| {
            part.timers
                .first_key_value()
                .map(|(key, _)| (owner, key.deadline))
        })
    }

    /// worker `k` の区画のいちばん早いタイマーの期限。輪が空なら `None` である。
    pub(crate) fn next_deadline(&self, k: usize) -> Option<u64> {
        self.parts[k]
            .timers
            .first_key_value()
            .map(|(key, _)| key.deadline)
    }

    /// worker `k` の区画で期限が `now_ms` までのタイマーを、期限の順、同じ期限なら登録の順に外し、待っていたタスクの
    /// 起こしを `out` に積む。輪の項と待ちの登録は、どちらが先に外れても他方を残さない (docs/spec/vm.md の「時間の待ち」)。
    /// 操作の待ちの期限は再開せず、driver への取り消しを `work` に積む。driver が記憶を使い終わるまで buffer を返せない
    /// ので、再開は結果を当てる `settle_op` が行う。予定したシグナルは、そのときの待ち手に届ける。
    pub(crate) fn expire(
        &mut self,
        now_ms: u64,
        k: usize,
        here: &Here,
        out: &mut Vec<Woken>,
        work: &mut IoWork,
    ) -> Result<(), CellFault> {
        while let Some(entry) = self.parts[k].timers.first_entry() {
            let key = *entry.key();
            if key.deadline > now_ms {
                break;
            }
            let cell = match entry.remove() {
                Due::Task(cell) => cell,
                Due::Signal(bit) => {
                    self.deliver(bit, out)?;
                    continue;
                }
            };
            let task = self.task_mut(cell)?;
            let parked = matches!(task.phase, Phase::Parked);
            if let (true, Some(Wait::Io(op))) = (parked, &task.wait) {
                let (op, owner) = (*op, task.owner);
                // 区画 `k` の項は、持ち主 `k` のタスクのものだけである
                debug_assert_eq!(owner as usize, k, "a timer is in another worker's part");
                let record = self.op_mut(op)?;
                if record.timer != Some(key) {
                    return Err(CellFault::Internal("a timer names no waiting task"));
                }
                record.timer = None;
                record.timed_out = true;
                self.cancel_op(op, here, work)?;
                continue;
            }
            if !parked || !matches!(task.wait, Some(Wait::Time(k)) if k == key) {
                return Err(CellFault::Internal("a timer names no waiting task"));
            }
            out.extend(self.wake_cell(cell, WOKE, None)?);
        }
        Ok(())
    }

    /// 待っているタスク `cell` の待ちを外し、`wake` で再開する起こしを返す。継続はセルに残し、持ち主が `take_wake` で
    /// 取り出す。止まっていないタスクには何もしない。
    pub(crate) fn wake_cell(
        &mut self,
        cell: CellRef,
        wake: i64,
        op: Option<CellRef>,
    ) -> Result<Option<Woken>, CellFault> {
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Parked) {
            return Ok(None);
        }
        task.wait = None;
        Ok(Some(Woken::Task { cell, wake, op }))
    }

    /// 起こし `w` を worker `me` が受け取る。持ち主が自分なら、セルから継続を取り出して走らせる仕事を返す。
    pub(crate) fn take_wake(&mut self, w: &Woken, me: u32) -> Result<Route, CellFault> {
        match *w {
            Woken::Token { owner, token } => Ok(if owner == me {
                Route::Run(Runnable::Token(token))
            } else {
                Route::Forward(owner)
            }),
            Woken::Task { cell, wake, op } => {
                let Some(CellKind::Task(task)) = self.live(cell) else {
                    return Ok(Route::Stale);
                };
                // 宛先はセルが今持つ持ち主で決める。起こしを作った後に持ち主が変わっても取り残さない
                if task.owner != me {
                    return Ok(Route::Forward(task.owner));
                }
                if !matches!(task.phase, Phase::Parked) || task.wait.is_some() {
                    return Ok(Route::Stale);
                }
                let Some(k) = task.k.take() else {
                    return Ok(Route::Stale);
                };
                Ok(Route::Run(match op {
                    None => Runnable::Resume {
                        cell,
                        worker: task.worker,
                        k,
                        wake,
                    },
                    Some(op) => Runnable::ResumeIo {
                        cell,
                        worker: task.worker,
                        k,
                        wake,
                        op: op.encode(),
                    },
                }))
            }
        }
    }

    /// 起こしをすべて worker `here.me` で受け取る。持ち主が自分のものは走らせる仕事にして `out` に積み、ほかの worker の
    /// ものは同じ区間でその持ち主の受け箱に送る。宛先は起こしを作ったときでなく、セルが今持つ持ち主で決める
    /// (docs/spec/vm.md の「セルと登録表」)。もう待っていないセルへの起こしは内部の誤りである。
    pub(crate) fn route(
        &mut self,
        woken: &mut Vec<Woken>,
        here: &Here,
        out: &mut Vec<Runnable>,
    ) -> Result<(), CellFault> {
        for w in woken.drain(..) {
            match self.take_wake(&w, here.me)? {
                Route::Run(run) => out.push(run),
                Route::Forward(owner) => here.post.wake(owner, w),
                Route::Stale => return Err(CellFault::Internal("a wake names no waiting task")),
            }
        }
        Ok(())
    }

    /// 受け箱の起こし `w` を worker `me` が受け取れば、走らせる仕事になるか。継続は取り出さない。run の終わりに残った
    /// 知らせを数えるために使う (docs/spec/vm.md の「誤り、止めること、終わり」)。
    pub(crate) fn would_run(&mut self, w: &Woken, me: u32) -> bool {
        match *w {
            Woken::Token { owner, .. } => owner == me,
            Woken::Task { cell, .. } => match self.live(cell) {
                Some(CellKind::Task(task)) => {
                    task.owner == me
                        && matches!(task.phase, Phase::Parked)
                        && task.wait.is_none()
                        && task.k.is_some()
                }
                _ => false,
            },
        }
    }

    /// 成功した run の `main` の `Finish` で、終わっていないタスクか、driver に出したままか置いたままの操作の記録が
    /// 残っているか。どれかがあれば、タスクが残ったまま run が終わる。結果を置いただけの記録は残ってよく、`sweep_all`
    /// が語を落とす (docs/spec/vm.md の「誤り、止めること、終わり」)。
    pub(crate) fn left_at_end(&self) -> bool {
        self.slots.iter().any(|(_, kind)| match kind {
            Some(CellKind::Task(task)) => !matches!(task.phase, Phase::Finished),
            Some(CellKind::Op(record)) => record.submitted || record.pending_op.is_some(),
            // 出したタスクは仕事を必ず取るか落とすので、成功した run の終わりに残る仕事のセルはない
            Some(CellKind::Job(_)) => true,
            _ => false,
        })
    }

    /// worker `k` の driver を静止させた後に、driver が返した完了を記録に置き、`k` が host の操作の数を 0 にする。
    /// 静止させた driver はもう記憶に触れないので、置いた結果は当てずに `sweep_all` が記録ごと捨てる
    /// (docs/spec/vm.md の「誤り、止めること、終わり」)。
    pub(crate) fn quiesced(&mut self, k: usize, completed: &mut Vec<Completion>) {
        for Completion {
            key,
            result,
            cancelled,
        } in completed.drain(..)
        {
            if let Some(CellKind::Op(record)) = self.live(CellRef::decode(key as i64))
                && record.submitted
            {
                record.submitted = false;
                record.arrival = Some((result, cancelled));
            }
        }
        self.parts[k].in_flight = 0;
    }

    /// 見張りの語を、待ち手の列が空でないシグナルに合わせる。中継しない run では 0 にする。登録表の lock の下で書くので、
    /// 複数の worker が書いても、最後に書いた値が列の今の形に合う (docs/spec/vm.md の「上限と割り込み」)。
    pub(crate) fn watch(&self, control: &Control) {
        control.set_watched(if self.relay { self.signal_bits() } else { 0 });
    }

    /// 待っていたタスク `cell` の持ち主の区画。
    fn part_of(&mut self, cell: CellRef) -> Result<&mut Part, CellFault> {
        let owner = self.task_mut(cell)?.owner as usize;
        Ok(&mut self.parts[owner])
    }
}

/// 旗の立ったタスク `cell` の届け先を決める。動いていて持ち主がほかの worker なら、持ち主へ `Notice::Limit` を回す。
/// 横取りされてフレームを持ったまま待つタスクも `Running` のままで、その `Task` を持つのは持ち主だからである。持ち主が
/// 自分ならセルを返し、呼び手の実行系がその VM の `Task` に旗を付ける。置いたか待っているタスクは VM の `Task` を
/// 持たないので、旗だけを残し、次の `started` か `resumed` が返す (docs/spec/vm.md の「タスクの上限」)。
fn route_limit(task: &TaskCell, cell: CellRef, here: &Here) -> Option<CellRef> {
    match task.phase {
        Phase::Running if task.owner != here.me => {
            here.post.limit(task.owner, cell);
            None
        }
        Phase::Running => Some(cell),
        Phase::Placed | Phase::Parked | Phase::Finished => None,
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
            CellKind::Scope(_) | CellKind::Chan(_) | CellKind::Op(_) | CellKind::Job(_) => Err(
                CellFault::Internal("a task cell reference names another kind of cell"),
            ),
        }
    }

    fn scope_mut(&mut self, cell: CellRef) -> Result<&mut ScopeCell, CellFault> {
        match self.cell_mut(cell)? {
            CellKind::Scope(scope) => Ok(scope),
            CellKind::Task(_) | CellKind::Chan(_) | CellKind::Op(_) | CellKind::Job(_) => Err(
                CellFault::Internal("a scope cell reference names another kind of cell"),
            ),
        }
    }

    fn job_mut(&mut self, cell: CellRef) -> Result<&mut JobCell, CellFault> {
        match self.cell_mut(cell)? {
            CellKind::Job(job) => Ok(job),
            CellKind::Task(_) | CellKind::Scope(_) | CellKind::Chan(_) | CellKind::Op(_) => Err(
                CellFault::Internal("a job cell reference names another kind of cell"),
            ),
        }
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

    /// スコープ `scope` に新しいタスクを置き、セルと、持ち主が自分なら始める仕事を返す。持ち主がほかの worker なら、
    /// 同じ区間でその受け箱に `Place` を送り、仕事は返さない。ハンドルは呼ぶ側が lock を放してから `task_handle` で作る。
    /// `spawner` は呼んだタスクの論理的な ID で、`owner` はタスクを動かす worker、`g` はエントリに渡す閉包である。
    /// 新しい ID に渡す前に、`g` とまだ印のないセッションの根 `roots` に共有の印を付ける (`par` の fork と同じ。
    /// docs/spec/vm.md の「`Async` のタスク」)。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        here: &Here,
        scope: CellRef,
        spawner: u32,
        owner: u32,
        g: Word,
        roots: &[Word],
    ) -> Result<Option<(CellRef, Option<Runnable>)>, CellFault> {
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
            owner,
            entries: 0,
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
            fast_yield: false,
            woke_from_job: false,
            run: None,
            detached: false,
            limit: false,
            bytes: 0,
        }));
        self.scope_mut(scope)?.members.push(cell);
        self.stats.tasks += 1;
        if owner != here.me {
            self.placed_tasks += 1;
            here.post.place(owner, cell, worker, g);
            return Ok(Some((cell, None)));
        }
        Ok(Some((
            cell,
            Some(Runnable::Start {
                cell,
                worker,
                closure: g,
            }),
        )))
    }

    /// 受け箱の `Place` が名指すタスクの今の持ち主。受けた worker は、自分なら始める仕事にし、ほかの worker ならその
    /// 持ち主へ送り直す。置いたタスクは始まるまで放されないので、始まるのを待っていないセルは内部の誤りである
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn placed_owner(&mut self, cell: CellRef, worker: u32) -> Result<u32, CellFault> {
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Placed) || task.worker != worker {
            return Err(CellFault::Internal("a placed task is not waiting to start"));
        }
        Ok(task.owner)
    }

    /// 始める仕事を実行系が動かし始めた。始めたタスクの ID とタスクの上限の印を返す。実行系は、その ID から閉包を
    /// `transfer_back` する。
    pub(crate) fn started(&mut self, cell: CellRef) -> Result<(u32, TaskMarks), CellFault> {
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Placed) {
            return Err(CellFault::Internal("a task starts twice"));
        }
        task.phase = Phase::Running;
        task.entries += 1;
        task.budget = BUDGET;
        let marks = TaskMarks {
            limit: task.limit,
            bytes: task.bytes,
        };
        Ok((task.spawner, marks))
    }

    /// 再開する仕事を実行系が動かし始めた。渡す `Wake` を返す。実行待ちから再開するタスクが取り消されていれば
    /// `Cancelled` に替える。操作の待ち (`io`) からの再開では、その待ちが予算を使い切ったときだけ予算を戻す。待ちが
    /// park するかは操作が受け付けで終わったかでスケジュールに依るので、park したかで戻すと予算の切れる位置と `parks`
    /// もスケジュールに依る (docs/spec/stdlib.md の「操作の予算」)。速い道の `reschedule` からの再開は取り消しを
    /// 数えない。std の包みはその `Wake` を見ず、次の取り消しの地点が数えて投げるためである。仕事の待ちからの再開は、
    /// 実行待ちからの再開と同じ規則で替える。仕事の待ちは取り消しで外れないので、取り消しは仕事が終わってから届く
    /// (docs/spec/vm.md の「取り消しの届け方」)。
    pub(crate) fn resumed(
        &mut self,
        cell: CellRef,
        wake: i64,
        io: bool,
    ) -> Result<(i64, TaskMarks), CellFault> {
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Parked) {
            return Err(CellFault::Internal("a task resumes without waiting"));
        }
        task.phase = Phase::Running;
        task.entries += 1;
        if !io || task.budget == 0 {
            task.budget = BUDGET;
        }
        let marks = TaskMarks {
            limit: task.limit,
            bytes: task.bytes,
        };
        let fast = std::mem::take(&mut task.fast_yield);
        let from_job = std::mem::take(&mut task.woke_from_job);
        let ready = matches!(task.wait.take(), Some(Wait::Ready));
        if (ready || from_job) && !fast && wake == WOKE && self.point(cell)? {
            self.stats.cancels += 1;
            return Ok((CANCELLED, marks));
        }
        Ok((wake, marks))
    }

    /// タスクの上限の旗を下ろし、タスクの数を 0 に戻す。`ResourceLimit` を起こした実行系が呼ぶ。もう放したセルは
    /// 何もしない。旗を見る者がもういないためである (docs/spec/vm.md の「タスクの上限」)。
    pub(crate) fn clear_limit(&mut self, cell: CellRef) -> Result<(), CellFault> {
        if self.live(cell).is_none() {
            return Ok(());
        }
        let task = self.task_mut(cell)?;
        task.limit = false;
        task.bytes = 0;
        Ok(())
    }

    /// mail の上限の旗の項を受けた。生きていて終わっていないタスクのセルに旗を立て、`route_limit` で届け先を決める。
    /// 死んだセルと終わったセルは何もしない。監視のスレッドが lock なしで読んだ古い番号でありうるためである。旗を
    /// 立てるのはこの道だけである (docs/spec/vm.md の「タスクの上限」)。
    pub(crate) fn limit_task(
        &mut self,
        cell: CellRef,
        here: &Here,
    ) -> Result<Option<CellRef>, CellFault> {
        let Some(CellKind::Task(task)) = self.live(cell) else {
            return Ok(None);
        };
        if matches!(task.phase, Phase::Finished) {
            return Ok(None);
        }
        task.limit = true;
        Ok(route_limit(task, cell, here))
    }

    /// ほかの worker が回した `Notice::Limit` を持ち主として受けた。旗をもう一度立てず、まだ立っているときだけ
    /// `route_limit` で届け先を決める。知らせが届く前にタスクが `ResourceLimit` を起こして旗を下ろしていれば、遅れた
    /// 知らせで2度目の失敗を起こさないためである (docs/spec/vm.md の「タスクの上限」)。
    pub(crate) fn limit_forwarded(&mut self, cell: CellRef, here: &Here) -> Option<CellRef> {
        match self.live(cell) {
            Some(CellKind::Task(task)) if task.limit && !matches!(task.phase, Phase::Finished) => {
                route_limit(task, cell, here)
            }
            _ => None,
        }
    }

    /// 待ちに入って `Finish` したエントリの `Task` の分だけ、セルごとに `entries` を1つ減らしてそのエントリの数を
    /// `bytes` に足し、`cells` を空にする。返すのは 0 になったセルの数である。持ち主が次の集める地点の最初の区間で呼ぶ。0 のセルを減らすのは内部の誤り
    /// である。もう放したセルは飛ばす。待ちに入ったエントリの `Task` が `Finish` に着く前に、同じタスクの次のエントリの
    /// `Task` がタスクを終え、ハンドルの `take` がセルを放しうるためである (docs/spec/vm.md の「救済」)。
    pub(crate) fn leave(&mut self, cells: &mut Vec<(CellRef, usize)>) -> Result<usize, CellFault> {
        let mut settled = 0;
        for (cell, added) in cells.drain(..) {
            if self.live(cell).is_none() {
                continue;
            }
            let task = self.task_mut(cell)?;
            task.entries = task.entries.checked_sub(1).ok_or(CellFault::Internal(
                "a task leaves an entry it has not entered",
            ))?;
            task.bytes = task.bytes.saturating_add(added);
            if task.entries == 0 {
                settled += 1;
            }
        }
        Ok(settled)
    }

    /// 救済する worker `me` が、worker `victim` の始める仕事のタスク `cell` を引き取る。まだ始まっておらず持ち主が
    /// `victim` なら、持ち主を `me` にして真を返す (docs/spec/vm.md の「救済」)。
    pub(crate) fn steal_start(
        &mut self,
        cell: CellRef,
        victim: u32,
        me: u32,
    ) -> Result<bool, CellFault> {
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Placed) || task.owner != victim {
            return Ok(false);
        }
        debug_assert_eq!(task.entries, 0, "a placed task has a live entry");
        task.owner = me;
        Ok(true)
    }

    /// 救済する worker `me` が、worker `victim` の実行待ちにある再開する仕事のタスク `cell` を引き取る。継続は仕事が
    /// 持つので、セルは継続も待ちの登録 (実行待ちのほか) も持たない。生きたエントリの `Task` がなく持ち主が `victim`
    /// なら、持ち主を `me` にして真を返す (docs/spec/vm.md の「救済」)。
    pub(crate) fn steal_resume(
        &mut self,
        cell: CellRef,
        victim: u32,
        me: u32,
    ) -> Result<bool, CellFault> {
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Parked)
            || !matches!(task.wait, None | Some(Wait::Ready))
            || task.k.is_some()
            || task.entries != 0
            || task.owner != victim
        {
            return Ok(false);
        }
        debug_assert_eq!(task.entries, 0, "a task with a live entry is rescued");
        task.owner = me;
        Ok(true)
    }

    /// 救済する worker `me` が、worker `victim` の受け箱の起こしのタスク `cell` を引き取る。起こされて継続をセルに
    /// 持ち、生きたエントリの `Task` がなく持ち主が `victim` なら、持ち主を `me` にして、取り出した継続とタスクの
    /// 論理的な ID を返す (docs/spec/vm.md の「救済」)。
    pub(crate) fn steal_wake(
        &mut self,
        cell: CellRef,
        victim: u32,
        me: u32,
    ) -> Result<Option<(Word, u32)>, CellFault> {
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Parked)
            || task.wait.is_some()
            || task.entries != 0
            || task.owner != victim
        {
            return Ok(None);
        }
        let Some(k) = task.k.take() else {
            return Ok(None);
        };
        debug_assert_eq!(task.entries, 0, "a task with a live entry is rescued");
        task.owner = me;
        Ok(Some((k, task.worker)))
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

    /// タスク `me` が、継続 `k` でセル `c` が終わるか閉じるのを待つ。もう終わっていれば、すぐ再開する仕事を返す。
    /// どちらでも、`k` は継続の待ちか返す仕事に移る。終わったセルにも近道をせず、いつも継続を渡して再開する。
    pub(crate) fn park(
        &mut self,
        c: i64,
        me: CellRef,
        k: Word,
    ) -> Result<Option<Runnable>, CellFault> {
        let target = CellRef::decode(c);
        if let CellKind::Job(_) = self.cell_mut(target)? {
            return self.park_job(target, me, k);
        }
        let done = match self.cell_mut(target)? {
            CellKind::Task(task) => matches!(task.phase, Phase::Finished),
            CellKind::Scope(scope) => scope.closed,
            CellKind::Chan(_) | CellKind::Op(_) | CellKind::Job(_) => {
                return Err(CellFault::Internal("a cell of the wrong kind"));
            }
        };
        let worker = self.suspend(me)?;
        self.stats.parks += 1;
        if self.point(me)? {
            self.stats.cancels += 1;
            return Ok(Some(Runnable::Resume {
                cell: me,
                worker,
                k,
                wake: CANCELLED,
            }));
        }
        if done {
            return Ok(Some(Runnable::Resume {
                cell: me,
                worker,
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
            CellKind::Chan(_) | CellKind::Op(_) | CellKind::Job(_) => {
                return Err(CellFault::Internal("a cell of the wrong kind"));
            }
        };
        waiters.push(Waiter::Task(me));
        Ok(None)
    }

    /// `park` の仕事の枝。取り消しの地点を先に見ない。仕事は取り消せず、取り消しは仕事が終わってから `resumed` が
    /// 届けるからである。終わった仕事では待ち手を登録せず、すぐ再開の項を返す (docs/spec/vm.md の
    /// 「取り消しの届け方」)。
    fn park_job(
        &mut self,
        target: CellRef,
        me: CellRef,
        k: Word,
    ) -> Result<Option<Runnable>, CellFault> {
        let job = self.job_mut(target)?;
        if job.submitter != me {
            return Err(CellFault::Internal(
                "a task waits for the job of another task",
            ));
        }
        let done = job.end.is_some();
        let worker = self.suspend(me)?;
        self.stats.parks += 1;
        let task = self.task_mut(me)?;
        if done {
            task.woke_from_job = true;
            return Ok(Some(Runnable::Resume {
                cell: me,
                worker,
                k,
                wake: WOKE,
            }));
        }
        task.k = Some(k);
        task.wait = Some(Wait::Job(target));
        self.job_mut(target)?.waiter = Some(me);
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
        let worker = self.suspend(me)?;
        self.stats.parks += 1;
        if self.point(me)? {
            self.stats.cancels += 1;
            return Ok(Some(Runnable::Resume {
                cell: me,
                worker,
                k,
                wake: CANCELLED,
            }));
        }
        let Some(target) = target else {
            return Ok(Some(Runnable::Resume {
                cell: me,
                worker,
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
                Waiter::Token { .. } => true,
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
        let worker = self.suspend(me)?;
        self.stats.parks += 1;
        self.task_mut(me)?.wait = Some(Wait::Ready);
        Ok(Runnable::Resume {
            cell: me,
            worker,
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
        let worker = self.suspend(me)?;
        self.stats.parks += 1;
        if self.point(me)? {
            self.stats.cancels += 1;
            return Ok(Some(Runnable::Resume {
                cell: me,
                worker,
                k,
                wake: CANCELLED,
            }));
        }
        if ms <= 0 {
            self.task_mut(me)?.wait = Some(Wait::Ready);
            return Ok(Some(Runnable::Resume {
                cell: me,
                worker,
                k,
                wake: WOKE,
            }));
        }
        // 期限は飽和させ、どんな長さも誤りにしない。仮想の時計が端に着いた後は、どの期限も今と等しくなる
        let key = self
            .part_of(me)?
            .add_timer(now_ms.saturating_add(ms as u64), Due::Task(me));
        self.stats.timers += 1;
        let task = self.task_mut(me)?;
        task.k = Some(k);
        task.wait = Some(Wait::Time(key));
        Ok(None)
    }

    /// タスク `me` が、継続 `k` でシグナル `signal` (0 が `Interrupt`、1 が `Terminate`) を待つ。取り消しの地点で、
    /// 待つ前に取り消されていれば列に積まずに `Cancelled` で再開する仕事を返す (docs/spec/vm.md の「待ちの2種類」)。
    pub(crate) fn park_signal(
        &mut self,
        signal: i64,
        me: CellRef,
        k: Word,
    ) -> Result<Option<Runnable>, CellFault> {
        // 番号は `Std.Async.signal_wait` が確かめて渡すので、ほかの値は検査を通ったプログラムからは届かない
        let index = usize::try_from(signal)
            .ok()
            .filter(|&i| i < SIGNAL_BITS.len())
            .ok_or(CellFault::Internal("a signal number is out of range"))?;
        let worker = self.suspend(me)?;
        self.stats.parks += 1;
        if self.point(me)? {
            self.stats.cancels += 1;
            return Ok(Some(Runnable::Resume {
                cell: me,
                worker,
                k,
                wake: CANCELLED,
            }));
        }
        self.signal_waiters[index].push(me);
        let task = self.task_mut(me)?;
        task.k = Some(k);
        task.wait = Some(Wait::Signal(index));
        Ok(None)
    }

    /// 待ち手の列が空でないシグナルのビット。
    pub(crate) fn signal_bits(&self) -> u32 {
        SIGNAL_BITS
            .iter()
            .zip(&self.signal_waiters)
            .filter(|(_, waiters)| !waiters.is_empty())
            .fold(0, |bits, (&bit, _)| bits | bit)
    }

    /// シグナルのビット `bits` を届ける。種類ごとに、待つタスクをすべて待ち始めた順に `Woke` で再開する起こしを `out` に
    /// 積み、列を空にする。待つタスクのいない種類は `unheard` に足す (docs/spec/stdlib.md の「`Std.Signal`」)。
    fn deliver(&mut self, bits: u32, out: &mut Vec<Woken>) -> Result<(), CellFault> {
        for (index, &bit) in SIGNAL_BITS.iter().enumerate() {
            if bits & bit == 0 {
                continue;
            }
            let waiters = std::mem::take(&mut self.signal_waiters[index]);
            if waiters.is_empty() {
                self.unheard |= bit;
            }
            for cell in waiters {
                let task = self.task_mut(cell)?;
                if !matches!(task.phase, Phase::Parked)
                    || !matches!(task.wait, Some(Wait::Signal(i)) if i == index)
                {
                    return Err(CellFault::Internal("a signal waiter is not waiting"));
                }
                out.extend(self.wake_cell(cell, WOKE, None)?);
            }
        }
        Ok(())
    }

    /// 動いているタスク `me` を止め、その論理的な ID を返す。すぐ再開する仕事はこの ID を持って戻る。
    fn suspend(&mut self, me: CellRef) -> Result<u32, CellFault> {
        let task = self.task_mut(me)?;
        if !matches!(task.phase, Phase::Running) {
            return Err(CellFault::Internal("a task waits without running"));
        }
        task.phase = Phase::Parked;
        Ok(task.worker)
    }

    /// `cell` が終わるか閉じるまで、worker `owner` の token で待つ。
    pub(crate) fn wait(
        &mut self,
        cell: CellRef,
        token: WaitToken,
        owner: u32,
    ) -> Result<(), CellFault> {
        let waiters = match self.cell_mut(cell)? {
            CellKind::Task(task) => &mut task.waiters,
            CellKind::Scope(scope) => &mut scope.waiters,
            CellKind::Chan(_) | CellKind::Op(_) | CellKind::Job(_) => {
                return Err(CellFault::Internal("a cell of the wrong kind"));
            }
        };
        waiters.push(Waiter::Token { token, owner });
        Ok(())
    }

    /// タスク `cell` を `outcome` で終える。起こしを `out` に積む。失敗はスコープの最初の失敗として記録する。
    /// スコープの最後のタスクなら、閉じる印のあるスコープを閉じ、加わるタスクを取り消す印を下ろし、スコープの待ち手を
    /// すべて起こす。`park_drain` の待ち手は閉じる印のないスコープも待つ。ハンドルがもう落ちていれば、セルをすぐ放し、
    /// 値の破棄と ID の退かせを `discards` に積む。
    pub(crate) fn finish(
        &mut self,
        here: &Here,
        cell: CellRef,
        outcome: Outcome,
        out: &mut Vec<Woken>,
        discards: &mut Discards,
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
            self.mark(here, Mark::Scope(scope))?;
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
            self.free_task(cell, discards)?;
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
        out: &mut Vec<Woken>,
    ) -> Result<(), CellFault> {
        match waiter {
            Waiter::Task(t) => {
                if let Some(CellKind::Task(task)) = self.live(t)
                    && matches!(task.wait, Some(Wait::Cell(c)) if c == from)
                {
                    out.extend(self.wake_cell(t, WOKE, None)?);
                }
            }
            Waiter::Token { token, owner } => out.push(Woken::Token { owner, token }),
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

    /// 終わったタスク `cell` の終わり方を取り出してセルを放す。値なら、その語と、呼ぶ側が受け取るときに使うタスクの ID を
    /// 返す。失敗なら起こし直す失敗を返す。根のスコープの失敗が先の `Take::Run` では、値の語を `dropped` に積む。
    /// タスクの ID の退かせは `retired` に積む。呼ぶ側は、値を受け取るか `dropped` を行い、ハンドルを手放してから
    /// `retired` を行う。
    pub(crate) fn take(
        &mut self,
        cell: CellRef,
        kind: Take,
        dropped: &mut Discards,
        retired: &mut Discards,
    ) -> Result<Result<(Word, u32), Raise>, CellFault> {
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
                    dropped.word(w, worker);
                }
                Err(raise(failure))
            }
            // 値はタスクが `finish_ok` に預けた参照で、受け取る側だけが持つ
            (None, Outcome::Value(w)) => Ok((w, worker)),
            (None, failure) => Err(raise(failure)),
        };
        self.free_task(cell, retired)?;
        Ok(result)
    }

    /// `detach` と `cancel_raw`。ハンドルを手放したタスクのセルを、終わっていれば放し、そうでなければ持ち手のないタスクに
    /// する。
    pub(crate) fn let_go(
        &mut self,
        here: &Here,
        cell: CellRef,
        cancel: bool,
        discards: &mut Discards,
    ) -> Result<(), CellFault> {
        let task = self.task_mut(cell)?;
        if matches!(task.phase, Phase::Finished) {
            return self.free_task(cell, discards);
        }
        task.detached = true;
        if cancel {
            self.mark(here, Mark::Task(cell))?;
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

    /// 速い道の予算を `cost` だけ使う。残りより多ければ残りをすべて使う。使い切ったら真を返し、再開まで真のままである。
    pub(crate) fn spend_budget(&mut self, me: CellRef, cost: u32) -> Result<bool, CellFault> {
        let task = self.task_mut(me)?;
        task.budget = task.budget.saturating_sub(cost);
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
    pub(crate) fn cancel_scope(&mut self, here: &Here, s: i64) -> Result<(), CellFault> {
        if let Some(cell) = self.scope_cell(s)
            && !self.scope_mut(cell)?.closed
        {
            self.mark(here, Mark::Scope(cell))?;
        }
        Ok(())
    }

    /// `race` の勝ちを決める。呼んだタスクのビットか今のスコープの旗が立っていれば、取り消しが届いたものとして数えて偽を
    /// 返す。そうでなければ、同じ呼び出しの中で `s` に旗を立てて真を返す。`shield` は見ない
    /// (docs/spec/vm.md の「命令」)。
    pub(crate) fn claim_scope(
        &mut self,
        here: &Here,
        s: i64,
        me: CellRef,
    ) -> Result<bool, CellFault> {
        let task = self.task_mut(me)?;
        let (bit, current) = (task.cancel, task.current);
        if bit || self.scope_mut(current)?.flag {
            self.stats.cancels += 1;
            return Ok(false);
        }
        self.cancel_scope(here, s)?;
        Ok(true)
    }

    /// `s` に属するタスクをそれぞれ取り消し、`s` が空になるまで加わるタスクも取り消す。`s` の旗は立てないので、`s` の
    /// 本体は取り消さない。印は一覧を読む前に立て、間に加わるタスクを取りこぼさない
    /// (docs/spec/vm.md の「取り消しの届け方」)。
    pub(crate) fn cancel_members(&mut self, here: &Here, s: i64) -> Result<(), CellFault> {
        if let Some(cell) = self.scope_cell(s)
            && !self.scope_mut(cell)?.closed
        {
            let scope = self.scope_mut(cell)?;
            scope.cancel_new = !scope.members.is_empty();
            let members = scope.members.clone();
            for m in members {
                self.mark(here, Mark::Task(m))?;
            }
        }
        Ok(())
    }

    /// `scope` の本体の失敗を記録し、子をすべて取り消す。種類 `Cancelled` は記録しない。
    pub(crate) fn fail_scope(
        &mut self,
        here: &Here,
        s: i64,
        failure: Outcome,
    ) -> Result<(), CellFault> {
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
        self.mark(here, Mark::Scope(cell))
    }

    /// 取り消しの印を付ける。ビットと旗を立てて取り消しを届けるだけで、待ちは外さない。作業の列でたどる。
    fn mark(&mut self, here: &Here, first: Mark) -> Result<(), CellFault> {
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
                    self.send_cancel(here, t)?;
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
                        self.send_cancel(here, o)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// タスク `t` の取り消しを持ち主に届ける。持ち主が自分なら mail に積み、ほかの worker ならその受け箱に送る。1つの
    /// worker の上では、取り消しも落ちたハンドルもシグナルも1つの mail に積んだ順に処理する (docs/spec/vm.md の
    /// 「取り消しの届け方」)。
    fn send_cancel(&mut self, here: &Here, t: CellRef) -> Result<(), CellFault> {
        let owner = self.task_mut(t)?.owner;
        if owner == here.me {
            here.mail.push(MailItem::Cancel {
                cell: t,
                owner: here.me,
            });
        } else {
            here.post.cancel(owner, t);
        }
        Ok(())
    }

    /// 届いた取り消しを処理する。持ち主が自分なら待ちを外し、ほかの worker なら、外さずにその持ち主へ送り直す。待ちを
    /// 外すことと操作の取り消しは持ち主だけが行うからである (docs/spec/vm.md の「セルと登録表」)。
    pub(crate) fn cancel_task(
        &mut self,
        cell: CellRef,
        here: &Here,
        out: &mut Vec<Woken>,
        work: &mut IoWork,
    ) -> Result<(), CellFault> {
        let Some(CellKind::Task(task)) = self.live(cell) else {
            return Ok(());
        };
        if task.owner != here.me {
            here.post.cancel(task.owner, cell);
            return Ok(());
        }
        self.unpark(cell, here, out, work)
    }

    /// 取り消しの項を受けたタスクの待ちを外す。タスクとスコープの待ち手の項は残し、起こす側が `wait` で見分ける。
    /// channel の待ち手の列の項はいつも生きているので、ここで外す。操作を待つタスクは、driver が記憶を使い終わるまで
    /// 再開できないので、待ちを外さずに取り消しを `work` に積み、結果を当てるときに `Cancelled` として再開する
    /// (docs/spec/vm.md の「取り消しの届け方」)。
    fn unpark(
        &mut self,
        cell: CellRef,
        here: &Here,
        out: &mut Vec<Woken>,
        work: &mut IoWork,
    ) -> Result<(), CellFault> {
        let me = here.me;
        let Some(CellKind::Task(task)) = self.live(cell) else {
            return Ok(());
        };
        if task.owner != me {
            return Err(CellFault::Internal("a task is unparked by another worker"));
        }
        if !matches!(task.phase, Phase::Parked)
            || !matches!(
                task.wait,
                Some(Wait::Cell(_) | Wait::Time(_) | Wait::Io(_) | Wait::Signal(_))
            )
        {
            return Ok(());
        }
        let io = match task.wait {
            Some(Wait::Io(op)) => Some(op),
            _ => None,
        };
        let owner = me;
        if !self.point(cell)? {
            return Ok(());
        }
        if let Some(op) = io {
            let record = self.op_mut(op)?;
            if !record.cancelling {
                record.cancelling = true;
                let (timed_out, timer) = (record.timed_out, record.timer.take());
                // 期限がもう取り消しを頼んでいれば、頼み直さない
                if !timed_out {
                    self.cancel_op(op, here, work)?;
                }
                // 輪の項を残すと、満期に `expire` が取り消した記録に期限が来た印を付ける
                if let Some(key) = timer {
                    self.parts[owner as usize].timers.remove(&key);
                }
                self.stats.cancels += 1;
            }
            return Ok(());
        }
        let task = self.task_mut(cell)?;
        match task.wait.take() {
            // 輪の項を残すと、満期に `expire` が待たないタスクを名指して誤りにする
            Some(Wait::Time(key)) => {
                self.parts[owner as usize].timers.remove(&key);
            }
            Some(Wait::Cell(c)) => {
                if let Some(chan) = self.chan_mut(c) {
                    chan.senders.retain(|&t| t != cell);
                    chan.receivers.retain(|&t| t != cell);
                }
            }
            Some(Wait::Signal(index)) => self.signal_waiters[index].retain(|&t| t != cell),
            Some(Wait::Ready | Wait::Io(_) | Wait::Job(_)) | None => {}
        }
        self.stats.cancels += 1;
        out.extend(self.wake_cell(cell, CANCELLED, None)?);
        Ok(())
    }

    /// mail の項を1つ処理する。落ちたハンドル、シグナル、上限の旗の項はどの worker が処理してもよく、取り消しの項は
    /// 持ち主の項だけを受け取る (`Mail::take`)。取り消しで待ちを外したタスクの起こしを `out` に、driver への取り消しを
    /// `work` に、放したセルの値と ID を `discards` に、旗を付ける VM の `Task` が自分の worker にあるタスクのセルを
    /// `limited` に積み、保留から下ろすシグナルのビットを返す。処理が新しい項を mail に積むことがある
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn receive_mail(
        &mut self,
        here: &Here,
        item: MailItem,
        out: &mut Vec<Woken>,
        work: &mut IoWork,
        discards: &mut Discards,
        limited: &mut Vec<CellRef>,
    ) -> Result<u32, CellFault> {
        match item {
            MailItem::HandleDropped(cell) => self.let_go(here, cell, true, discards)?,
            MailItem::Cancel { cell, owner } => {
                debug_assert_eq!(owner, here.me, "a cancel is taken by another worker");
                self.cancel_task(cell, here, out, work)?;
            }
            MailItem::Signal(bits) => {
                self.deliver(bits, out)?;
                return Ok(bits);
            }
            MailItem::Limit(cell) => limited.extend(self.limit_task(cell, here)?),
        }
        Ok(0)
    }

    /// run の終わりに、残ったセルをすべて放し、語の破棄と ID の退かせを `discards` に積む。閉じていない channel と品物の
    /// 残った channel もここで放す。driver が保留の操作を持たないことを前提にし、呼ぶ側が先に `quiesce` する
    /// (docs/spec/vm.md の「時計と driver」)。操作の記録の語は受け付けたタスクの ID の物体なので、タスクのセルが ID を
    /// 退かせる前に、記録を先に回って落とす (docs/spec/vm.md の「終わり」)。
    pub(crate) fn sweep_all(&mut self, discards: &mut Discards) -> Result<(), CellFault> {
        for index in 0..self.slots.len() {
            if matches!(self.slots[index].1, Some(CellKind::Op(_)))
                && let Some(CellKind::Op(record)) = self.slots[index].1.take()
            {
                // 預かる ID は記録のものではなく、タスクのセルか `main` のもので、`free_task` が退かせる
                for w in [record.buf, record.stream, record.made]
                    .into_iter()
                    .flatten()
                {
                    discards.word(w, record.worker);
                }
                self.release(index as u32);
            }
        }
        // 呼ぶ側が先に `quiesce` したので、driver が捨てた完了は当てない
        for part in &mut self.parts {
            part.in_flight = 0;
        }
        for index in 0..self.slots.len() {
            let (generation, kind) = &self.slots[index];
            let cell = CellRef {
                index: index as u32,
                generation: *generation,
            };
            match kind {
                Some(CellKind::Task(_)) => self.free_task(cell, discards)?,
                Some(CellKind::Scope(_)) => self.release(cell.index),
                Some(CellKind::Chan(_)) => self.free_chan(cell, discards)?,
                Some(CellKind::Job(_)) => self.free_job(cell, discards)?,
                Some(CellKind::Op(_)) | None => {}
            }
        }
        // 眠るタスクを残して終わる run がある (`exit` や誤り) ので、空であることは確かめない
        for part in &mut self.parts {
            part.timers.clear();
        }
        for waiters in &mut self.signal_waiters {
            waiters.clear();
        }
        Ok(())
    }

    /// タスクのセルを放す。受け取られなかった値をタスクの ID の下で落とし、ID を退かせることを `discards` に積む。
    /// 退かせるときに ID の物体が残っていれば、`par` の join と同じく `TransferLeak` がヒープの誤りとして出る。
    fn free_task(&mut self, cell: CellRef, discards: &mut Discards) -> Result<(), CellFault> {
        let task = self.task_mut(cell)?;
        let (worker, outcome) = (task.worker, task.outcome.take());
        if let Some(Outcome::Value(w)) = outcome {
            discards.word(w, worker);
        }
        discards.retire(worker);
        self.release(cell.index);
        Ok(())
    }
}

/// `compute` の仕事のセル (docs/spec/vm.md の「`compute` の仕事」)。`fork_job` の行が作り、仕事の終わりが
/// `finish_job` で終わりを置いて待ち手を起こし、出したタスクの `take_job` か `drop_job` の行が放す。
impl Registry {
    /// タスク `submitter` が出した、論理的な ID `worker` で動く仕事のセルを作る。
    pub(crate) fn new_job(&mut self, worker: u32, submitter: CellRef) -> CellRef {
        self.alloc(CellKind::Job(JobCell {
            worker,
            submitter,
            end: None,
            waiter: None,
        }))
    }

    /// 仕事 `cell` の終わり `end` を置き、待っているタスクがいれば起こしを `out` に積む。
    pub(crate) fn finish_job(
        &mut self,
        cell: CellRef,
        end: JobEnd,
        out: &mut Vec<Woken>,
    ) -> Result<(), CellFault> {
        let job = self.job_mut(cell)?;
        if job.end.is_some() {
            return Err(CellFault::Internal("a job ends twice"));
        }
        job.end = Some(end);
        let Some(t) = job.waiter.take() else {
            return Ok(());
        };
        let task = self.task_mut(t)?;
        if !matches!(task.wait, Some(Wait::Job(c)) if c == cell) {
            return Err(CellFault::Internal(
                "a job wakes a task that does not wait for it",
            ));
        }
        task.woke_from_job = true;
        out.extend(self.wake_cell(t, WOKE, None)?);
        Ok(())
    }

    /// タスク `me` が token `raw` の終わった仕事を取り出し、セルを放す。仕事の ID と終わりを返す。終わりの語は仕事の ID
    /// の物体なので、呼ぶ側が lock を放してから `transfer_back` し、ID を退かせる。
    pub(crate) fn take_job(&mut self, raw: i64, me: CellRef) -> Result<(u32, JobEnd), CellFault> {
        let cell = CellRef::decode(raw);
        let job = self.job_mut(cell)?;
        if job.submitter != me {
            return Err(CellFault::Internal("a task takes the job of another task"));
        }
        if job.end.is_none() {
            return Err(CellFault::Internal("a job is taken before it ends"));
        }
        let Some(CellKind::Job(job)) = self.slots[cell.index as usize].1.take() else {
            unreachable!("the job cell is live");
        };
        self.release(cell.index);
        let end = job.end.expect("the end is checked above");
        Ok((job.worker, end))
    }

    /// run の終わりに残った仕事のセルを放す。終わりの語を仕事の ID の下で落とし、ID を退かせることを `discards` に積む。
    fn free_job(&mut self, cell: CellRef, discards: &mut Discards) -> Result<(), CellFault> {
        let job = self.job_mut(cell)?;
        let (worker, end) = (job.worker, job.end.take());
        if let Some(end) = end {
            discards.word(end.word(), worker);
        }
        discards.retire(worker);
        self.release(cell.index);
        Ok(())
    }
}

/// driver の操作の記録 (docs/spec/vm.md の「操作の記録」)。受け付けの行が buffer とストリームを記録に移して
/// token を返し、`park_io` が待ち、結果を当てる `settle_op` が再開し、取り出しの行が記録を放す。driver の結果は、
/// 受け付けでその場で終わったものと集める地点の完了の2つの道から届き、どちらも host が `arrive` で記録に置いてから
/// 持ち主が `settle_op` で当てる。
impl Registry {
    fn op_mut(&mut self, op: CellRef) -> Result<&mut OpRecord, CellFault> {
        match self.cell_mut(op)? {
            CellKind::Op(record) => Ok(record),
            CellKind::Task(_) | CellKind::Scope(_) | CellKind::Chan(_) | CellKind::Job(_) => Err(
                CellFault::Internal("an op reference names another kind of cell"),
            ),
        }
    }

    /// token `raw` が指す記録。token を受ける行はどれもここを通る。放した記録と前の run の記録は generation で見分ける。
    /// 記録に触れるのは受け付けたタスク `worker` だけである。速いモードでは worker ID がいつも 0 なので、この比べは
    /// 検査モードでだけ効く (docs/spec/vm.md の「操作の記録」)。
    fn op_of(&mut self, raw: i64, worker: u32) -> Result<CellRef, CellFault> {
        let op = CellRef::decode(raw);
        let Some(CellKind::Op(record)) = self.live(op) else {
            return Err(CellFault::Internal("an op names no record"));
        };
        if record.worker != worker {
            return Err(CellFault::Internal("an op is used by another task"));
        }
        Ok(op)
    }

    /// 記録を表に入れ、記録の番号を token として返す。持ち主は worker `here.me` で、host は呼ぶ側が記録に書いてある。
    /// `op` があれば記録に置き、host が自分ならすぐ取り出して `work` に積み、ほかの worker なら host へ `Submit` を
    /// 送る。driver の操作を出さずに終わった記録は `op` を持たない (docs/spec/vm.md の「操作の記録」)。
    fn record(
        &mut self,
        mut record: OpRecord,
        op: Option<Op>,
        here: &Here,
        work: &mut IoWork,
    ) -> Result<i64, CellFault> {
        record.owner = here.me;
        let cell = self.alloc(CellKind::Op(record));
        if let Some(op) = op {
            self.submit_op(cell, op, here, work)?;
        }
        Ok(cell.encode())
    }

    /// 持ち主 `here.me` が作った操作 `op` を記録 `c` に置く。host が自分なら取り出して `work` に積み、ほかの worker
    /// なら同じ区間で host の受け箱に `Submit` を送る。
    fn submit_op(
        &mut self,
        c: CellRef,
        op: Op,
        here: &Here,
        work: &mut IoWork,
    ) -> Result<(), CellFault> {
        self.queue_op(c, op)?;
        let record = self.op_mut(c)?;
        if record.owner != here.me {
            return Err(CellFault::Internal("an op is submitted by another worker"));
        }
        let host = record.host;
        if host == here.me {
            let (op, cancel_now) = self.take_queued(c)?;
            work.submits.push((c, op, cancel_now));
        } else {
            here.post.submit(host, c);
        }
        Ok(())
    }

    /// 持ち主が、記録 `c` に次の操作 `op` を置く。記録は driver に出しておらず、置いた操作もない。
    pub(crate) fn queue_op(&mut self, c: CellRef, op: Op) -> Result<(), CellFault> {
        let record = self.op_mut(c)?;
        if record.submitted || record.pending_op.is_some() || record.arrival.is_some() {
            return Err(CellFault::Internal("an op is queued while it is busy"));
        }
        record.pending_op = Some(op);
        Ok(())
    }

    /// host が、記録 `c` に置かれた操作を driver に出すために取り出す。同じ区間で受け付けた印を立て、取り消しの印を読む。
    /// 真なら、取り消しが受け付けの前に来たので、host は出した後に取り消しを頼む (docs/spec/vm.md の「操作の記録」)。
    pub(crate) fn take_queued(&mut self, c: CellRef) -> Result<(Op, bool), CellFault> {
        let record = self.op_mut(c)?;
        let op = record.pending_op.take().ok_or(CellFault::Internal(
            "an op is submitted without being queued",
        ))?;
        record.submitted = true;
        let (host, cancel_now) = (record.host, record.cancelling || record.timed_out);
        self.parts[host as usize].in_flight += 1;
        Ok((op, cancel_now))
    }

    /// host `me` が、driver から受け取った記録 `c` の結果を置く。記録は driver に出したもので、まだ結果を置いていない。
    /// そうでない鍵の結果は内部の誤りである。結果を当てるのは持ち主なので、持ち主がほかの worker なら、待つタスクが
    /// いるときだけ `Completion` を送らせ、いなければ持ち主が次に待つか続けるときに当てる (docs/spec/vm.md の「操作の記録」)。
    pub(crate) fn arrive(
        &mut self,
        c: CellRef,
        result: io::Result<Done>,
        cancelled: bool,
        me: u32,
    ) -> Result<Arrival, CellFault> {
        let record = match self.live(c) {
            Some(CellKind::Op(record)) if record.submitted => record,
            _ => return Err(CellFault::Internal("a completion names no op")),
        };
        record.submitted = false;
        record.arrival = Some((result, cancelled));
        let (host, owner, waiting) = (record.host, record.owner, record.waiter.is_some());
        self.parts[host as usize].in_flight -= 1;
        Ok(match (owner == me, waiting) {
            (true, _) => Arrival::Settle,
            (false, true) => Arrival::Send(owner),
            (false, false) => Arrival::Leave,
        })
    }

    /// 持ち主 `me` が、記録 `c` の driver の操作を取り消す道を決める。取り消しの印 (`cancelling` か `timed_out`) は
    /// 呼ぶ側が先に立てる。まだ driver に出していなければ、host が出す区間で印を読むので、ここでは何も頼まない。
    pub(crate) fn cancel_io(&mut self, c: CellRef, me: u32) -> Result<CancelPath, CellFault> {
        let record = self.op_mut(c)?;
        Ok(match (record.submitted, record.host) {
            (false, _) => CancelPath::Flag,
            (true, host) if host == me => CancelPath::Local,
            (true, host) => CancelPath::Remote(host),
        })
    }

    /// 持ち主 `here.me` が、記録 `op` の取り消しを頼む。自分が host なら `work` に積み、ほかの worker なら同じ区間で
    /// host の受け箱に `CancelIo` を送る。
    fn cancel_op(&mut self, op: CellRef, here: &Here, work: &mut IoWork) -> Result<(), CellFault> {
        match self.cancel_io(op, here.me)? {
            CancelPath::Flag => {}
            CancelPath::Local => work.cancels.push(op),
            CancelPath::Remote(host) => here.post.cancel_io(host, op),
        }
        Ok(())
    }

    /// host `me` が、受け箱の `Submit` を受ける。持ち主が置いた操作を取り出し、lock を放してから driver に出すよう
    /// `work` に積む (docs/spec/vm.md の「操作の記録」)。
    pub(crate) fn take_submit(
        &mut self,
        op: CellRef,
        me: u32,
        work: &mut IoWork,
    ) -> Result<(), CellFault> {
        if self.op_mut(op)?.host != me {
            return Err(CellFault::Internal("an op is submitted to another host"));
        }
        let (next, cancel_now) = self.take_queued(op)?;
        work.submits.push((op, next, cancel_now));
        Ok(())
    }

    /// host `me` が、受け箱の `CancelIo` を受ける。記録がまだ driver に出したままなら、lock を放してから自分の driver に
    /// 取り消しを頼むよう `work` に積む。結果をもう置いた記録と放した記録には何もしない。持ち主が送った区間の後に、
    /// driver の完了が先に届きうるからである。host でない worker への `CancelIo` は内部の誤りである。
    pub(crate) fn cancel_hosted(
        &mut self,
        op: CellRef,
        me: u32,
        work: &mut IoWork,
    ) -> Result<(), CellFault> {
        if let Some(CellKind::Op(record)) = self.live(op) {
            if record.host != me {
                return Err(CellFault::Internal("an op cancel is sent to another host"));
            }
            if record.submitted {
                work.cancels.push(op);
            }
        }
        Ok(())
    }

    /// 持ち主 `here.me` が、受け箱の `Completion` を受けて記録 `op` の結果を当てる。host は待つタスクがいるときだけ
    /// 送り、待つタスクは結果を受けるまで止まっているので、結果は `Completion` だけが当てる。生きていない記録と結果の
    /// ない記録は内部の誤りである (docs/spec/vm.md の「操作の記録」)。
    pub(crate) fn take_completion<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        depth_offset: usize,
        op: CellRef,
        here: &Here,
        work: &mut IoWork,
    ) -> Result<Option<Woken>, CellFault> {
        match self.live(op) {
            Some(CellKind::Op(record)) if record.arrival.is_some() => {}
            _ => return Err(CellFault::Internal("a completion names no arrived op")),
        }
        self.settle_arrived(ctx, depth_offset, op, here, work)
    }

    /// 持ち主 `here.me` が、記録 `op` に置かれた結果があれば当てる。当てて記録が終われば、待つタスクの起こしを返す。
    /// 結果を置いていない記録には何もしない (docs/spec/vm.md の「操作の記録」)。
    pub(crate) fn settle_arrived<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        depth_offset: usize,
        op: CellRef,
        here: &Here,
        work: &mut IoWork,
    ) -> Result<Option<Woken>, CellFault> {
        let record = self.op_mut(op)?;
        if record.arrival.is_none() {
            return Ok(None);
        }
        let host = record.host;
        let bin = self
            .bins
            .get(host as usize)
            .cloned()
            .flatten()
            .ok_or(CellFault::Internal("an op has a host without a driver"))?;
        self.settle_op(ctx, &bin, depth_offset, op, here, work)
    }

    /// タスク `me` が token `token` の操作を待つか続けるかを決める前に、host が置いた結果があれば当てる。決める区間と
    /// 同じ区間で当てるので、結果の置かれた記録を終わっていないと見て待ち、`Completion` を受けずに止まることはない
    /// (docs/spec/vm.md の「操作の記録」)。
    pub(crate) fn settle_token<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        depth_offset: usize,
        token: i64,
        me: CellRef,
        here: &Here,
        work: &mut IoWork,
    ) -> Result<(), CellFault> {
        let worker = self.task_mut(me)?.worker;
        let op = self.op_of(token, worker)?;
        // 待つタスクはまだいないので、当てても起こしはない
        match self.settle_arrived(ctx, depth_offset, op, here, work)? {
            Some(_) => Err(CellFault::Internal("an op is awaited twice")),
            None => Ok(()),
        }
    }

    /// 速い道の `Std.Async.op_ready`。タスク `me` が token `token` の操作を待たずに続けてよいかを答える。どの待ちでも
    /// 予算を使い、書きは長さに応じて多く使う。記録が終わっていて取り消しの地点が投げなければ、予算が残れば待ちを
    /// 1つ数えて `FAST_GO`、切れれば数えずに `FAST_YIELD` を返す。`FAST_YIELD` の後は、呼ぶ側が OS を見てから譲るか
    /// `go_on` で続けるかを決める。ほかは `FAST_WAIT` で、待つ側の `park_io` か `park_io_until` が今の規則で数える。
    /// 地点を読むだけで、取り消しは数えない (docs/spec/vm.md の「操作の記録」)。
    pub(crate) fn op_ready(&mut self, token: i64, ms: i64, me: CellRef) -> Result<i64, CellFault> {
        let worker = self.task_mut(me)?.worker;
        let op = self.op_of(token, worker)?;
        let record = self.op_mut(op)?;
        if record.waiter.is_some() {
            return Err(CellFault::Internal("an op is awaited twice"));
        }
        let done = matches!(record.phase, OpPhase::Done(..));
        let cost = record.kind.cost();
        // 予算は、操作が受け付けで終わったかによらず待ちごとに使う。予算の切れる位置をプログラムだけで決めるためで
        // ある。使い切った待ちは、park するか、先に譲るか、譲らずに続けるかして、予算を戻す
        // (docs/spec/stdlib.md の「操作の予算」)
        let spent = self.spend_budget(me, cost)?;
        if !done || self.point(me)? {
            return Ok(FAST_WAIT);
        }
        // 期限は、終わっていた操作でも `park_io_until` と同じく数える。数をスケジュールによらず決めるためである
        // (docs/spec/vm.md の「`RunStats`」)
        if ms > 0 {
            self.stats.timers += 1;
        }
        if spent {
            // 待ちは、続く `reschedule` の `ready` が1つ数える
            self.task_mut(me)?.fast_yield = true;
            return Ok(FAST_YIELD);
        }
        self.stats.parks += 1;
        Ok(FAST_GO)
    }

    /// `op_ready` が `FAST_YIELD` を返した後、OS を見ても走れるタスクがなかった。譲らずに続けるので、譲る印を下ろし、
    /// 譲った待ちの再開と同じく予算を戻して、続く `reschedule` の代わりに待ちを1つ数える。譲っても続けても予算の
    /// 切れる位置と `parks` が同じになるためである (docs/spec/stdlib.md の「操作の予算」)。
    pub(crate) fn go_on(&mut self, me: CellRef) -> Result<i64, CellFault> {
        let task = self.task_mut(me)?;
        if !std::mem::take(&mut task.fast_yield) {
            return Err(CellFault::Internal("a task goes on without a fast yield"));
        }
        task.budget = BUDGET;
        self.stats.parks += 1;
        Ok(FAST_GO)
    }

    /// タスク `me` が、継続 `k` で token `token` の操作が終わるまで待つ。token は即値で、再開の仕事が継続に返す。
    /// 終わっていれば、すぐ再開する仕事を返す。取り消しの地点で、終わっていなければ driver への取り消しを `work` に
    /// 積んで完了を待つ。driver が記憶を使い終わるまで、buffer を返せないためである。取り消しの地点なら、終わっていても
    /// いなくても、記録に取り消しを頼んだ印を付ける。
    pub(crate) fn park_io(
        &mut self,
        here: &Here,
        work: &mut IoWork,
        token: i64,
        me: CellRef,
        k: Word,
    ) -> Result<Option<Runnable>, CellFault> {
        self.park_op(here, work, token, me, k, None)
    }

    /// `park_io` に `ms` ミリ秒の期限を付けた待ち。`now_ms` は切り上げて読んだ今の時刻で、`ms` が 0 以下なら読まない。
    /// 期限は実際に待つときだけ付ける。0 以下は過ぎた期限なので、すぐ driver への取り消しを積む。期限が来ても再開せず、
    /// 取り消した操作の完了が `Woke` で再開する (docs/spec/vm.md の「時間の待ち」)。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn park_io_until(
        &mut self,
        here: &Here,
        work: &mut IoWork,
        now_ms: u64,
        ms: i64,
        token: i64,
        me: CellRef,
        k: Word,
    ) -> Result<Option<Runnable>, CellFault> {
        self.park_op(here, work, token, me, k, Some((now_ms, ms)))
    }

    fn park_op(
        &mut self,
        here: &Here,
        work: &mut IoWork,
        token: i64,
        me: CellRef,
        k: Word,
        until: Option<(u64, i64)>,
    ) -> Result<Option<Runnable>, CellFault> {
        let (worker, owner) = {
            let task = self.task_mut(me)?;
            (task.worker, task.owner)
        };
        if owner != here.me {
            return Err(CellFault::Internal("a task waits on another worker"));
        }
        let op = self.op_of(token, worker)?;
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
        // 投げずに包みへ返すので、包みが結果によらず `Cancelled` を投げる (docs/spec/stdlib.md の「`Std.Net`」)
        record.cancelling |= cancelled;
        if matches!(record.phase, OpPhase::Done(..)) {
            let wake = if cancelled { CANCELLED } else { WOKE };
            return Ok(Some(Runnable::ResumeIo {
                cell: me,
                worker,
                k,
                wake,
                op: token,
            }));
        }
        record.waiter = Some(me);
        if cancelled {
            self.cancel_op(op, here, work)?;
        }
        match until {
            Some((_, ms)) if !cancelled && ms <= 0 => {
                self.op_mut(op)?.timed_out = true;
                self.cancel_op(op, here, work)?;
            }
            Some((now_ms, ms)) if !cancelled => {
                // 期限は `park_time` と同じく飽和させる
                let key = self.parts[owner as usize]
                    .add_timer(now_ms.saturating_add(ms as u64), Due::Task(me));
                self.op_mut(op)?.timer = Some(key);
            }
            _ => {}
        }
        let task = self.task_mut(me)?;
        task.k = Some(k);
        task.wait = Some(Wait::Io(op));
        Ok(None)
    }

    /// 持ち主 `here.me` が、記録 `op` に置かれた結果を当てる。読みの `Probe` の結果は、buffer を貸して `Transfer` の
    /// 操作を同じ鍵で出し直す。host が自分ならその操作を `work` に積み、ほかの worker なら host へ送る。終わった記録に
    /// 待つタスクがいれば、その起こしを返す。記録の語は受け付けたタスクの物体なので、今のタスクが誰でも、その ID に
    /// 切り替えてから触れる。`bin` は host の driver の、作ったハンドルを閉じる bin である。
    pub(crate) fn settle_op<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        bin: &Arc<HostBin>,
        depth_offset: usize,
        op: CellRef,
        here: &Here,
        work: &mut IoWork,
    ) -> Result<Option<Woken>, CellFault> {
        let record = self.op_mut(op)?;
        // 記録の語に触れて段階を進めるのは持ち主だけである (docs/spec/vm.md の「操作の記録」)
        if record.owner != here.me {
            return Err(CellFault::Internal("an op is settled by another worker"));
        }
        let (result, cancelled) = record
            .arrival
            .take()
            .ok_or(CellFault::Internal("an op settles without a result"))?;
        self.stats.completions += 1;
        match self.advance(ctx, bin, depth_offset, op, result, cancelled)? {
            Some(next) => {
                self.submit_op(op, next, here, work)?;
                Ok(None)
            }
            None => self.wake_op(op),
        }
    }

    /// 1つの結果で記録の段階を進める。続けて driver に出す操作があればそれを返し、記録が終わったら `None` を返す。
    fn advance<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        bin: &Arc<HostBin>,
        depth_offset: usize,
        op: CellRef,
        result: io::Result<Done>,
        mut cancelled: bool,
    ) -> Result<Option<Op>, CellFault> {
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
            // 行の連鎖では、記録の buffer でなくストリームの buffer を広げて貸す
            (OpPhase::Probe, OpKind::Line { .. }, Ok(Done::Size(_))) => {
                let (worker, stream) = (record.worker, record.stream);
                let stream = stream.ok_or(CellFault::Internal("a read op has no stream"))?;
                let (handle, lent) = with_worker(ctx, worker, depth_offset, |ctx| {
                    lend_line_room(ctx, stream, true)
                })?;
                let (filled, raw) =
                    lent.ok_or(CellFault::Internal("a grown buffer has no room"))?;
                let record = self.op_mut(op)?;
                record.kind.set_line(filled, false);
                record.phase = OpPhase::Transfer;
                return Ok(Some(Op::Read { handle, buf: raw }));
            }
            (OpPhase::Probe, _, Ok(Done::Size(_))) => {
                let (worker, buf, stream) = (record.worker, record.buf, record.stream);
                let (Some(buf), Some(stream)) = (buf, stream) else {
                    return Err(CellFault::Internal("a read op has no buffer or stream"));
                };
                let (buf, handle, raw) = with_worker(ctx, worker, depth_offset, |ctx| {
                    grow_read_room(ctx, buf, stream)
                })?;
                let record = self.op_mut(op)?;
                record.buf = Some(buf);
                record.phase = OpPhase::Transfer;
                return Ok(Some(Op::Read { handle, buf: raw }));
            }
            // 取り消しを頼んだ読みも、外が書いたバイトは確定してから buffer を返す
            (OpPhase::Transfer, OpKind::Read { .. }, Ok(Done::Size(n))) => {
                let (worker, buf) = (record.worker, record.buf);
                let buf = buf.ok_or(CellFault::Internal("a read op has no buffer"))?;
                // 記録は貸した後も buffer の唯一の参照を持ち、書き換えていない。driver は `Ok(n)` で `n` バイトを
                // 書き終えている (docs/spec/object-model.md の T10 の行、docs/spec/vm.md の「操作の記録」)
                with_worker(ctx, worker, depth_offset, |ctx| unsafe {
                    ctx.heap_mut().bytes_commit(buf, n)
                })?;
                Ok(n)
            }
            (
                OpPhase::Transfer,
                OpKind::Write { .. }
                | OpKind::ShutdownWrite
                | OpKind::SetNodelay
                | OpKind::Wait { .. },
                Ok(Done::Size(n)),
            ) => Ok(n),
            // 箱は開けずに持つ。型を知るのは、仕事を受け付けた行の取り出しだけである
            (OpPhase::Transfer, OpKind::Job { .. }, Ok(Done::Job(value))) => {
                let OpKind::Job { answer, .. } = &mut self.op_mut(op)?.kind else {
                    unreachable!("the kind is matched above");
                };
                *answer = Some(value);
                Ok(0)
            }
            // 外が書いたバイトは、取り消しを頼んだ記録でもストリームの buffer に確定する。行の途中で止めても、
            // 読んだバイトを次の読みに残すためである (docs/spec/stdlib.md の「`Std.Net`」)
            (OpPhase::Transfer, &OpKind::Line { filled, .. }, result) => {
                let (worker, stream) = (record.worker, record.stream);
                let stopped = record.cancelling || record.timed_out;
                let stream = stream.ok_or(CellFault::Internal("a read op has no stream"))?;
                let got = match &result {
                    Ok(Done::Size(n)) => Some(*n),
                    Ok(_) => {
                        return Err(CellFault::Internal(
                            "an op completes with another kind of result",
                        ));
                    }
                    Err(_) => None,
                };
                let step = with_worker(ctx, worker, depth_offset, |ctx| {
                    line_step(ctx, stream, filled, got, stopped)
                })?;
                match (step, result) {
                    // 次の周も、ストリームの buffer に空きが残っていれば待たずに貸す。ソケットでないストリームは
                    // 空きがなければ広げて貸す
                    (LineStep::Again(handle), _) => {
                        let (_, lent) = with_worker(ctx, worker, depth_offset, |ctx| {
                            lend_line_room(ctx, stream, false)
                        })?;
                        let record = self.op_mut(op)?;
                        return Ok(Some(match lent {
                            Some((filled, raw)) => {
                                record.kind.set_line(filled, false);
                                Op::Read { handle, buf: raw }
                            }
                            // 貸さないのはソケットだけである (`lend_line_room`)
                            None => {
                                record.phase = OpPhase::Probe;
                                Op::Readable { handle }
                            }
                        }));
                    }
                    (LineStep::Line(line, text), _) => {
                        made = Some(line);
                        self.op_mut(op)?.kind.set_line(0, !text);
                        Ok(0)
                    }
                    // 取り消しか期限で止めた行は、取り消しだけで終わった完了と同じに扱う。読んだバイトは
                    // ストリームの buffer に残っている
                    (LineStep::Stopped, _) => {
                        cancelled = true;
                        Err(io::Error::other("cancelled"))
                    }
                    (LineStep::TooLong, _) => Err(line_too_long()),
                    (LineStep::Failed, result) => result.map(|_| 0),
                }
            }
            // 完了の中身のハンドルは、ここで記録のタスクの物体にする。取り消しを頼んだ記録でも作り、取り出しか
            // `finish` が落とすと、ハンドルは bin を通して閉じる (docs/spec/vm.md の「操作の記録」)
            (OpPhase::Transfer, OpKind::Listen { .. }, Ok(Done::Listener { handle, port })) => {
                let listener = NetListener {
                    bind: Bind::Bound {
                        host: record.host,
                        handle,
                        bin: Arc::clone(bin),
                    },
                    port,
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
                let stream = IoStream {
                    bind: Bind::Bound {
                        host: record.host,
                        handle,
                        bin: Arc::clone(bin),
                    },
                    origin: Origin::Peer(peer),
                    rest: Vec::new(),
                    start: 0,
                };
                made = Some(with_worker(ctx, record.worker, depth_offset, |ctx| {
                    // `NetListener` と同じ。`TcpStream` は `Lin` である
                    unsafe { ctx.heap_mut() }.alloc_foreign(stream, true)
                }));
                Ok(0)
            }
            // 受けたソケットは縛らずにストリームにする。最初に操作を受け付ける worker が自分の driver に取り込む
            // (docs/spec/vm.md の「操作の記録」)
            (OpPhase::Transfer, OpKind::Accept, Ok(Done::Accepted { sock, peer })) => {
                let stream = IoStream {
                    bind: Bind::Unbound(RawResource::Socket(sock)),
                    origin: Origin::Peer(peer),
                    rest: Vec::new(),
                    start: 0,
                };
                made = Some(with_worker(ctx, record.worker, depth_offset, |ctx| {
                    // `NetListener` と同じ。`TcpStream` は `Lin` である
                    unsafe { ctx.heap_mut() }.alloc_foreign(stream, true)
                }));
                Ok(0)
            }
            // 取り込んだファイルも `connect` の結果と同じく host に縛ったストリームにする。取り消しを頼んだ記録でも
            // 作り、包みの `finish` が落とすとハンドルは host の bin に積まれる (docs/spec/vm.md の「操作の記録」)
            (OpPhase::Transfer, OpKind::Open { path, .. }, Ok(Done::Handle(handle))) => {
                let stream = IoStream {
                    bind: Bind::Bound {
                        host: record.host,
                        handle,
                        bin: Arc::clone(bin),
                    },
                    origin: Origin::File(Arc::clone(path)),
                    rest: Vec::new(),
                    start: 0,
                };
                made = Some(with_worker(ctx, record.worker, depth_offset, |ctx| {
                    // `NetListener` と同じ。`Reader` と `Writer` は `Lin` である
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
        Ok(None)
    }

    /// 終わった記録 `op` の期限を外し、待つタスクがいればその起こしを返す。
    fn wake_op(&mut self, op: CellRef) -> Result<Option<Woken>, CellFault> {
        let record = self.op_mut(op)?;
        // 終わった操作の期限は要らない。輪の項を残すと、満期に `expire` が待たないタスクを名指す
        let timer = record.timer.take();
        let waiter = record.waiter;
        let wake = if record.cancelling { CANCELLED } else { WOKE };
        let Some(t) = waiter else {
            return Ok(None);
        };
        if let Some(key) = timer {
            self.part_of(t)?.timers.remove(&key);
        }
        let task = self.task_mut(t)?;
        if !matches!(task.wait, Some(Wait::Io(o)) if o == op) {
            return Err(CellFault::Internal("an op names no waiting task"));
        }
        self.wake_cell(t, wake, Some(op))
    }

    /// `Std.Net.op_stream` と `op_listener`。借りた token の終わった記録から、受け付けに渡した
    /// ストリームかリスナーを移し出す。
    pub(crate) fn op_stream(&mut self, token: i64, worker: u32) -> Result<Word, CellFault> {
        let op = self.op_of(token, worker)?;
        let record = self.op_mut(op)?;
        if !matches!(record.phase, OpPhase::Done(..)) {
            return Err(CellFault::Internal("an op is taken before its completion"));
        }
        record
            .stream
            .take()
            .ok_or(CellFault::Internal("an op has no stream to give back"))
    }

    /// `Std.Net.op_done`。借りた token の記録が終わっているか。待ちの地点を通らずに読むだけで、予算も取り消しも
    /// 見ない。受け付けで終わる操作の包みが、持ち主が host のときに待たずに続けるためである
    /// (docs/spec/stdlib.md の「`Std.Net`」)。
    pub(crate) fn op_done(&mut self, token: i64, worker: u32) -> Result<bool, CellFault> {
        let op = self.op_of(token, worker)?;
        Ok(matches!(self.op_mut(op)?.phase, OpPhase::Done(..)))
    }

    /// `Std.Net.op_outcome`。借りた token の終わった記録の結果の番号 (docs/spec/vm.md の「操作の記録」)。
    pub(crate) fn op_outcome(&mut self, token: i64, worker: u32) -> Result<i64, CellFault> {
        let op = self.op_of(token, worker)?;
        let record = self.op_mut(op)?;
        match &record.phase {
            OpPhase::Done(result, cancelled) => {
                Ok(outcome(&record.kind, result, record.timed_out, *cancelled))
            }
            OpPhase::Probe | OpPhase::Transfer => {
                Err(CellFault::Internal("an op is taken before its completion"))
            }
        }
    }

    /// `Std.Stream.op_prefix`。借りた token の終わった記録が、誤りの文言の前に付ける語。包みは番号 2 から 5 の失敗の
    /// 文言にこれを使う (docs/spec/stdlib.md の「`Std.Stream`」)。
    pub(crate) fn op_prefix(&mut self, token: i64, worker: u32) -> Result<String, CellFault> {
        let op = self.op_of(token, worker)?;
        let record = self.op_mut(op)?;
        if !matches!(record.phase, OpPhase::Done(..)) {
            return Err(CellFault::Internal("an op is taken before its completion"));
        }
        Ok(record.kind.prefix().into_owned())
    }

    /// token を消費して終わった記録を放し、結果と、記録が持っていたストリームを返す。ストリームは呼ぶ側が lock を放して
    /// から落とす。取り出すのは受け付けたタスク (`worker`) だけである。
    fn take_op(&mut self, token: i64, worker: u32) -> Result<(Taken, Option<Word>), CellFault> {
        let op = self.op_of(token, worker)?;
        // 記録の持ち主 (`owner`) はわざと確かめない。救済で移った譲りのタスクは、元の worker を持ち主にした終わった記録を
        // 持ったまま、救済した worker で取り出すためである (docs/spec/vm.md の「救済」)
        let record = self.op_mut(op)?;
        if !matches!(record.phase, OpPhase::Done(..)) {
            return Err(CellFault::Internal("an op is taken before its completion"));
        }
        let Some(CellKind::Op(record)) = self.slots[op.index as usize].1.take() else {
            unreachable!("the op cell is live");
        };
        self.release(op.index);
        let OpPhase::Done(result, cancelled) = record.phase else {
            unreachable!("the phase is checked above");
        };
        let taken = Taken {
            result,
            buf: record.buf,
            made: record.made,
            kind: record.kind,
            cancelling: record.cancelling,
            timed_out: record.timed_out,
            cancelled,
        };
        Ok((taken, record.stream))
    }
}

/// 取り出しの行。token を消費して終わった記録を放し、結果と buffer を返す。残ったストリームは落とす。取り出すのは
/// 受け付けたタスク (`worker`) だけである。記録の語はそのタスクの物体なので、切り替えずに受け取れる。
pub(crate) fn take_op<M: Mode>(
    shared: &Shared,
    ctx: &mut Ctx<M>,
    token: i64,
    worker: u32,
) -> Result<Taken, CellFault> {
    let (taken, stream) = shared.lock().take_op(token, worker)?;
    // ストリームは記録が持っていた参照で、受け付けたタスクの ID の物体である。そのタスクだと `op_of` で確かめた。
    // 操作は終わっているので、driver はもう記憶に触れない (docs/spec/vm.md の「操作の記録」)
    if let Some(stream) = stream {
        unsafe { ctx.heap_mut().decref(stream) }?;
    }
    Ok(taken)
}

/// 取り出しの行が受け取る、完了の中身から作った `Foreign` の種類。
#[derive(Clone, Copy)]
pub(crate) enum Made {
    /// `Std.Net.take_listener`。
    Listener,
    /// `Std.Net.take_stream`。
    Stream,
    /// `Std.Stream.take_reader`。
    Reader,
    /// `Std.Stream.take_writer`。
    Writer,
}

/// `Std.Net.take_listener`、`take_stream`、`Std.Stream.take_reader`、`take_writer`。token を消費して記録を放し、
/// 完了の中身から作った `Foreign` を返す。結果の番号が `OUTCOME_OK` の記録だけを取り出す。`made` は求める記録の種類で、
/// 違う種類の記録は内部の誤りである。
pub(crate) fn take_made<M: Mode>(
    shared: &Shared,
    ctx: &mut Ctx<M>,
    token: i64,
    worker: u32,
    made: Made,
) -> Result<Word, CellFault> {
    let taken = take_op(shared, ctx, token, worker)?;
    if taken.outcome() != OUTCOME_OK {
        return Err(CellFault::Internal("an op is taken after it failed"));
    }
    let kind_matches = match made {
        Made::Listener => matches!(taken.kind, OpKind::Listen { .. }),
        Made::Stream => matches!(taken.kind, OpKind::Accept | OpKind::Connect { .. }),
        Made::Reader => matches!(taken.kind, OpKind::Open { write: false, .. }),
        Made::Writer => matches!(taken.kind, OpKind::Open { write: true, .. }),
    };
    match taken.made {
        Some(made) if kind_matches => Ok(made),
        _ => Err(CellFault::Internal("an op is taken as another kind of op")),
    }
}

/// `Std.Net.take_bytes`。token を消費して記録を放し、記録の buffer を返す。読みなら読んだバイトを足した buffer、
/// 書きなら書いた buffer そのものである。結果の番号が `OUTCOME_OK` の読みか書きの記録だけを取り出す
/// (docs/spec/stdlib.md の「`Std.Net`」)。
pub(crate) fn take_bytes<M: Mode>(
    shared: &Shared,
    ctx: &mut Ctx<M>,
    token: i64,
    worker: u32,
) -> Result<Word, CellFault> {
    let taken = take_op(shared, ctx, token, worker)?;
    if taken.outcome() != OUTCOME_OK {
        return Err(CellFault::Internal("an op is taken after it failed"));
    }
    match taken.buf {
        Some(buf) if matches!(taken.kind, OpKind::Read { .. } | OpKind::Write { .. }) => Ok(buf),
        _ => Err(CellFault::Internal("an op is taken as another kind of op")),
    }
}

/// `Std.Net.take_line`。token を消費して記録を放し、完了の時点で写した行 (終わりの改行を含み、残りのない EOF なら空)
/// の文字列を返す。行が UTF-8 でなければ、写した `Bytes` を落として IO の失敗の文言を返す。結果の番号が `OUTCOME_OK`
/// の行の記録だけを取り出す (docs/spec/stdlib.md の「`Std.Net`」)。
pub(crate) fn take_line<M: Mode>(
    shared: &Shared,
    ctx: &mut Ctx<M>,
    token: i64,
    worker: u32,
) -> Result<Result<Word, String>, CellFault> {
    let (line, text, prefix) = take_line_op(shared, ctx, token, worker)?;
    if text {
        return Ok(Ok(line));
    }
    // 写した `Bytes` は取り出した記録が持っていた唯一の参照で、今の worker の物体である
    unsafe { ctx.heap_mut().decref(line) }?;
    Ok(Err(format!("{prefix}: the line is not valid UTF-8")))
}

/// `Std.Stream.take_line_bytes`。`take_line` と同じ行を、UTF-8 かどうかによらず `Bytes` で返す
/// (docs/spec/stdlib.md の「`Std.Stream`」)。
pub(crate) fn take_line_bytes<M: Mode>(
    shared: &Shared,
    ctx: &mut Ctx<M>,
    token: i64,
    worker: u32,
) -> Result<Word, CellFault> {
    let (line, text, _) = take_line_op(shared, ctx, token, worker)?;
    if !text {
        return Ok(line);
    }
    // 文字列は記録が持っていた唯一の参照なので、`bytes_of_string` はその場で `Bytes` に変えられる
    Ok(unsafe { ctx.bytes_of_string(line) }?)
}

/// 行の記録を取り出し、写した行の物体、それが文字列であること、誤りの文言の前に付ける語を返す。
fn take_line_op<M: Mode>(
    shared: &Shared,
    ctx: &mut Ctx<M>,
    token: i64,
    worker: u32,
) -> Result<(Word, bool, String), CellFault> {
    let taken = take_op(shared, ctx, token, worker)?;
    if taken.outcome() != OUTCOME_OK {
        return Err(CellFault::Internal("an op is taken after it failed"));
    }
    match (&taken.kind, taken.made) {
        (OpKind::Line { not_utf8, .. }, Some(line)) => {
            Ok((line, !*not_utf8, taken.kind.prefix().into_owned()))
        }
        _ => Err(CellFault::Internal("an op is taken as another kind of op")),
    }
}

/// `Std.Net.submit_write`。`buf` と `stream` は呼ぶ側から移ってきた参照で、記録に移す。
/// `buf` の内容をすべて書く操作を driver に出し、token を返す。`me` は呼んだ worker、`worker` は呼んだタスクの論理的な
/// ID で、`depth_offset` はその深さの起点である。共有の `Bytes` も写さずに貸し、`take_bytes` が同じ参照を返す。
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_write<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    mut driver: Option<&mut (dyn IoDriver + 'static)>,
    worker: u32,
    depth_offset: usize,
    buf: Word,
    stream: Word,
) -> Result<i64, CellFault> {
    let bound = bind_stream(shared, here, ctx, driver.as_deref_mut(), stream)?;
    // 記録が物体を生かし、取り出すまで書き換えないので、driver は完了までこの位置を読める。一意は求めない。
    // その場で書き換える操作はどれも一意を求め、記録が参照を1つ持つ間は写すからである
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
    let kind = OpKind::Write {
        len: raw.cap,
        prefix: stream_prefix(ctx, stream)?,
    };
    let mut record = net_record(kind, worker, Some(stream));
    record.buf = Some(buf);
    let op = match bound {
        Ok(handle) => Some(Op::Write { handle, buf: raw }),
        Err(e) => {
            record.phase = OpPhase::Done(Err(e), false);
            None
        }
    };
    submit(shared, here, ctx, driver, depth_offset, record, op, None)
}

/// `Std.Net.submit_read`。`buf` と `stream` を `submit_write` と同じく記録に移す。buffer が一意で空きが
/// `MIN_ROOM` バイト以上あれば、その空きを貸して読む操作を出す。そうでなければ、端が読めるようになるのを待つ操作を
/// 出し、読めるようになってから広げて貸す。何も来ない端の読みに、呼び手が渡さない記憶を持たせないためである
/// (docs/spec/vm.md の「操作の記録」)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_read<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    mut driver: Option<&mut (dyn IoDriver + 'static)>,
    worker: u32,
    depth_offset: usize,
    buf: Word,
    stream: Word,
) -> Result<i64, CellFault> {
    let bound = bind_stream(shared, here, ctx, driver.as_deref_mut(), stream)?;
    // 一意でない buffer はここで写さず、広げるときに1回だけ写す。`read_line` が読みすぎたバイトがあれば、driver の
    // 操作を出さずにそれを返す (docs/spec/stdlib.md の「`Std.Net`」)
    // 読めるようになるのを待つのはソケットだけで、ほかのストリームは受け付けで広げて貸す。ファイルの読みは
    // プールの仕事なので、読めるかを問う操作がない (docs/spec/vm.md の「操作の記録」)
    let socket = unsafe { (*ctx.heap().foreign_unique::<IoStream>(stream)?).is_socket() };
    let (buf, phase, op) = match (serve_rest(ctx, buf, stream)?, bound) {
        (Some((buf, n)), _) => (buf, OpPhase::Done(Ok(n), false), None),
        (None, Err(e)) => (buf, OpPhase::Done(Err(e), false), None),
        (None, Ok(handle)) => match spare_room(ctx, buf)? {
            Some(raw) => (buf, OpPhase::Transfer, Some(Op::Read { handle, buf: raw })),
            None if socket => (buf, OpPhase::Probe, Some(Op::Readable { handle })),
            None => {
                let (buf, handle, raw) = grow_read_room(ctx, buf, stream)?;
                (buf, OpPhase::Transfer, Some(Op::Read { handle, buf: raw }))
            }
        },
    };
    let kind = OpKind::Read {
        prefix: stream_prefix(ctx, stream)?,
    };
    let mut record = net_record(kind, worker, Some(stream));
    record.buf = Some(buf);
    record.phase = phase;
    submit(shared, here, ctx, driver, depth_offset, record, op, None)
}

/// `Std.Net.submit_read_line`。`stream` は呼ぶ側から移ってきた参照で、記録に移す。ストリームの buffer の残りに改行が
/// あれば、driver の操作を出さずに行を切り出して終わった記録にする。なければ行の連鎖を始める。buffer に空きが
/// あればそれを貸して読み、なければストリームが読めるようになるのを待つ (docs/spec/vm.md の「操作の記録」)。
pub(crate) fn submit_read_line<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    mut driver: Option<&mut (dyn IoDriver + 'static)>,
    worker: u32,
    depth_offset: usize,
    stream: Word,
) -> Result<i64, CellFault> {
    let bound = bind_stream(shared, here, ctx, driver.as_deref_mut(), stream)?;
    // ストリームは `TcpStream` の `Foreign` で、呼ぶ側から移ってきた唯一の参照である (`TcpStream` は `Lin`)
    let s = unsafe { &*ctx.heap().foreign_unique::<IoStream>(stream)? };
    let found = s.find_line(0, false);
    let (mut filled, mut made, mut not_utf8) = (0, None, false);
    let (op, phase) = match found {
        Some(Found::Line(n)) => {
            let (line, text) = take_line_text(ctx, stream, n)?;
            (made, not_utf8) = (Some(line), !text);
            (None, OpPhase::Done(Ok(0), false))
        }
        Some(_) => (None, OpPhase::Done(Err(line_too_long()), false)),
        None => match bound {
            Err(e) => (None, OpPhase::Done(Err(e), false)),
            Ok(_) => match lend_line_room(ctx, stream, false)? {
                (handle, Some((at, raw))) => {
                    filled = at;
                    (Some(Op::Read { handle, buf: raw }), OpPhase::Transfer)
                }
                // 貸さないのはソケットだけである (`lend_line_room`)
                (handle, None) => (Some(Op::Readable { handle }), OpPhase::Probe),
            },
        },
    };
    let kind = OpKind::Line {
        filled,
        not_utf8,
        prefix: stream_prefix(ctx, stream)?,
    };
    let mut record = net_record(kind, worker, Some(stream));
    record.phase = phase;
    record.made = made;
    submit(shared, here, ctx, driver, depth_offset, record, op, None)
}

/// `Std.Net.submit_listen`。`host` と `port` は呼ぶ側から移ってきた参照と即値で、`host` は読んでから手放す。
/// `host` が IP アドレスの綴りでないか `port` が範囲の外なら、操作を出さずに IO の失敗の文言を返す
/// (docs/spec/stdlib.md の「`Std.Net`」)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_listen<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    driver: Option<&mut (dyn IoDriver + 'static)>,
    worker: u32,
    depth_offset: usize,
    host: Word,
    port: i64,
) -> Result<Result<i64, String>, CellFault> {
    let host = take_string(ctx, host)?;
    let prefix = format!("cannot listen on {host}:{port}");
    let Ok(ip) = host.parse::<IpAddr>() else {
        return Ok(Err(format!("{prefix}: the host is not an IP address")));
    };
    let Ok(port) = u16::try_from(port) else {
        return Ok(Err(format!("{prefix}: the port is out of range")));
    };
    let op = Op::Listen {
        addr: SocketAddr::new(ip, port),
    };
    let kind = OpKind::Listen {
        prefix: prefix.into(),
    };
    let record = net_record(kind, worker, None);
    Ok(Ok(submit(
        shared,
        here,
        ctx,
        driver,
        depth_offset,
        record,
        Some(op),
        None,
    )?))
}

/// `Std.Net.submit_accept`。リスナー `listener` は呼ぶ側から移ってきた参照で、記録に移す。
pub(crate) fn submit_accept<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    driver: Option<&mut (dyn IoDriver + 'static)>,
    worker: u32,
    depth_offset: usize,
    listener: Word,
) -> Result<i64, CellFault> {
    // リスナーは `TcpListener` の `Foreign` で、呼ぶ側から移ってきた唯一の参照である (`TcpListener` は `Lin`)
    let handle = unsafe {
        (*ctx.heap().foreign_unique::<NetListener>(listener)?)
            .bind
            .handle()?
    };
    let op = Op::Accept { listener: handle };
    let record = net_record(OpKind::Accept, worker, Some(listener));
    submit(
        shared,
        here,
        ctx,
        driver,
        depth_offset,
        record,
        Some(op),
        None,
    )
}

/// `Std.Net.submit_connect`。`submit_listen` と同じく `host` を読んで手放す。`host` は名前でもよく、driver が解決する。
/// `port` が 1 から 65535 の外なら、操作を出さずに IO の失敗の文言を返す (docs/spec/stdlib.md の「`Std.Net`」)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_connect<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    driver: Option<&mut (dyn IoDriver + 'static)>,
    worker: u32,
    depth_offset: usize,
    host: Word,
    port: i64,
) -> Result<Result<i64, String>, CellFault> {
    let host = take_string(ctx, host)?;
    let prefix = format!("cannot connect to {host}:{port}");
    let port = match u16::try_from(port) {
        Ok(port) if port > 0 => port,
        _ => return Ok(Err(format!("{prefix}: the port is out of range"))),
    };
    let op = Op::Connect { host, port };
    let kind = OpKind::Connect {
        prefix: prefix.into(),
    };
    let record = net_record(kind, worker, None);
    Ok(Ok(submit(
        shared,
        here,
        ctx,
        driver,
        depth_offset,
        record,
        Some(op),
        None,
    )?))
}

/// `Std.Stream.submit_open`。`path` は呼ぶ側から移ってきた参照で、読んでから手放す。`mode` は 0 が読み、1 が作る、2 が
/// 足すである。Host が呼んだ worker の上でファイルを開き、開けなければ記録を作らずに、誤りと IO の失敗の文言を返す。
/// 開けたファイルは `Op::Adopt` で呼んだタスクの I/O の host の driver に取り込み、`connect` と同じく完了から
/// ストリームを作る (docs/spec/vm.md の「操作の記録」)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_open<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    driver: Option<&mut (dyn IoDriver + 'static)>,
    worker: u32,
    depth_offset: usize,
    path: Word,
    mode: i64,
) -> Result<Result<i64, (io::Error, String)>, CellFault> {
    let path = take_string(ctx, path)?;
    let mode = match mode {
        0 => OpenMode::Read,
        1 => OpenMode::Create,
        2 => OpenMode::Append,
        _ => return Err(CellFault::Internal("a file is opened in an unknown mode")),
    };
    let file = match ctx.host().open_raw(&path, mode) {
        Ok(file) => file,
        Err(e) => {
            let message = if eml_rt::is_not_regular(&e) {
                format!("cannot open `{path}`: {}", eml_rt::NOT_REGULAR)
            } else {
                Fault::Io {
                    op: eml_rt::IoOp::Open,
                    name: path.into(),
                    reason: eml_rt::io_reason(&e),
                }
                .to_string()
            };
            return Ok(Err((e, message)));
        }
    };
    let kind = OpKind::Open {
        path: path.into(),
        write: mode != OpenMode::Read,
    };
    let op = Op::Adopt {
        res: RawResource::File(file),
    };
    let record = net_record(kind, worker, None);
    Ok(Ok(submit(
        shared,
        here,
        ctx,
        driver,
        depth_offset,
        record,
        Some(op),
        None,
    )?))
}

/// `Std.Net.submit_shutdown_write` と `submit_set_nodelay`。`stream` は呼ぶ側から移ってきた参照で、記録に移す。
/// driver を直接呼ばず、ほかの操作と同じく記録を通して host の driver に出す。持ち主が host でない worker でも、
/// 同じ道で host に届くためである (docs/spec/vm.md の「操作の記録」)。`on` は `set_nodelay` の値で、`None` なら
/// `shutdown_write` である。
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_control<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    mut driver: Option<&mut (dyn IoDriver + 'static)>,
    worker: u32,
    depth_offset: usize,
    on: Option<bool>,
    stream: Word,
) -> Result<i64, CellFault> {
    let bound = bind_stream(shared, here, ctx, driver.as_deref_mut(), stream)?;
    let kind = match on {
        None => OpKind::ShutdownWrite,
        Some(_) => OpKind::SetNodelay,
    };
    let mut record = net_record(kind, worker, Some(stream));
    let op = match (bound, on) {
        (Ok(handle), None) => Some(Op::ShutdownWrite { handle }),
        (Ok(handle), Some(on)) => Some(Op::SetNodelay { handle, on }),
        (Err(e), _) => {
            record.phase = OpPhase::Done(Err(e), false);
            None
        }
    };
    submit(shared, here, ctx, driver, depth_offset, record, op, None)
}

/// 呼ぶ側の物体から lock の外で作った記録を表に入れ、`op` があれば driver に出して、token を返す。driver がその場で
/// 終えた操作はここで決着させる。誤りで終わっても記録は終わった記録として残り、取り出しの行が放す
/// (docs/spec/vm.md の「操作の記録」)。`point` のタスクが取り消しの地点で投げるなら、`op` を出さずに取り消しで
/// 終わった記録にする (docs/spec/vm.md の「`blocking` の呼び出し」)。
#[allow(clippy::too_many_arguments)]
fn submit<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    driver: Option<&mut (dyn IoDriver + 'static)>,
    depth_offset: usize,
    mut record: OpRecord,
    mut op: Option<Op>,
    point: Option<CellRef>,
) -> Result<i64, CellFault> {
    // 縛ったハンドルを引く操作だけが、縛った worker を host にする。取り込みに失敗したストリームの記録は driver に出さない
    record.host = match record.stream {
        Some(w) => bound_host(ctx, &record.kind, w)?.unwrap_or(here.host),
        None => here.host,
    };
    let mut work = IoWork::default();
    let token = {
        let mut cells = shared.lock();
        // 地点の確かめと記録を1つの区間に置き、取り消しが間に入って仕事だけが始まることをなくす。`stats` と予算と
        // `parks` には触れない。後の `io_wait` が、終わった記録を待つ取り消しの地点として数える
        if let Some(me) = point
            && cells.point(me)?
        {
            record.phase = OpPhase::Done(Err(io::Error::other("cancelled")), true);
            record.cancelling = true;
            op = None;
        }
        cells.record(record, op, here, &mut work)?
    };
    let mut out = Vec::new();
    run_io(shared, here, ctx, driver, depth_offset, &mut work, &mut out)?;
    // 待つタスクはまだいないので、再開の仕事はできない
    if !out.is_empty() {
        return Err(CellFault::Internal("a new op has a waiting task"));
    }
    Ok(token)
}

/// 登録表の lock の外で、`work` に並べた受け付けを順に host の driver に出し、その後で取り消しを頼む。driver がその場で
/// 終えた操作は、1回ごとに短い lock の区間で結果を置く。持ち主が自分ならその区間で当て、当てた結果が次の操作を出せば、
/// それも続けて出す。持ち主がほかの worker なら、待つタスクがいるときに `Completion` を送る。終わった操作の待つ
/// 自分のタスクを再開する仕事を `out` に積む (docs/spec/vm.md の「操作の記録」)。driver を持たない worker は自分で
/// 受け付けないので、出す操作も取り消しもないときだけ `driver` が `None` でよい。
pub(crate) fn run_io<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    driver: Option<&mut (dyn IoDriver + 'static)>,
    depth_offset: usize,
    work: &mut IoWork,
    out: &mut Vec<Runnable>,
) -> Result<(), CellFault> {
    let Some(driver) = driver else {
        return work.cancel_on(None);
    };
    while !work.submits.is_empty() {
        for (op, next, cancel_now) in std::mem::take(&mut work.submits) {
            let Submit::Done(result) = driver.submit(op_key(op), next) else {
                // 取り出す前に取り消しが来ていた。終わった操作は取り消さない
                if cancel_now {
                    driver.cancel(op_key(op));
                }
                continue;
            };
            let raw = driver.bin();
            let mut cells = shared.lock();
            let bin = cells.host_bin(here.me, raw)?;
            settle_arrival(
                &mut cells,
                here,
                ctx,
                &bin,
                depth_offset,
                op,
                result,
                false,
                work,
                out,
            )?;
        }
    }
    for op in work.cancels.drain(..) {
        driver.cancel(op_key(op));
    }
    Ok(())
}

/// lock の区間の中で、host の `here.me` が記録 `op` の結果を置く。持ち主も自分なら同じ区間で当て、待つタスクの起こしを
/// 走らせる仕事にして `out` に積む。持ち主がほかの worker なら、待つタスクがいるときだけ持ち主へ `Completion` を送る。
/// host は記録の語に触れない (docs/spec/vm.md の「操作の記録」)。
#[allow(clippy::too_many_arguments)]
fn settle_arrival<M: Mode>(
    cells: &mut Registry,
    here: &Here,
    ctx: &mut Ctx<M>,
    bin: &Arc<HostBin>,
    depth_offset: usize,
    op: CellRef,
    result: io::Result<Done>,
    cancelled: bool,
    work: &mut IoWork,
    out: &mut Vec<Runnable>,
) -> Result<(), CellFault> {
    match cells.arrive(op, result, cancelled, here.me)? {
        Arrival::Settle => {}
        Arrival::Send(owner) => {
            here.post.completion(owner, op);
            return Ok(());
        }
        Arrival::Leave => return Ok(()),
    }
    // 待つタスクは記録の持ち主のタスクで、持ち主は自分なので、送り直す起こしはない
    if let Some(woken) = cells.settle_op(ctx, bin, depth_offset, op, here, work)? {
        match cells.take_wake(&woken, here.me)? {
            Route::Run(run) => out.push(run),
            Route::Forward(_) => {
                return Err(CellFault::Internal("an op is awaited on another worker"));
            }
            Route::Stale => return Err(CellFault::Internal("a wake names no waiting task")),
        }
    }
    Ok(())
}

/// 集める地点で、host の `here.me` の driver の完了を driver の順に記録へ置き、自分が持ち主の記録は当てて、待つタスクを
/// 再開する仕事を `out` に積む。ほかの worker が持ち主の記録は、その持ち主へ `Completion` を送る。driver に出した操作が
/// あるときと、ほかの worker が落としたハンドルを driver に閉じさせるときだけ呼ぶ。driver は完了を問われたときに bin を
/// 空にするためである。数は集める地点がほかの読みと同じ区間で読む。完了は1つの lock の区間で続けて置き、当てた結果が
/// 次の操作を出すときだけ lock を放して出す。出した操作がその場で終われば、次の完了より先に当てる
/// (docs/spec/vm.md の「操作の記録」)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn complete<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    driver: &mut (dyn IoDriver + 'static),
    depth_offset: usize,
    completed: &mut Vec<Completion>,
    out: &mut Vec<Runnable>,
) -> Result<(), CellFault> {
    driver.completions(completed);
    if completed.is_empty() {
        return Ok(());
    }
    let raw = driver.bin();
    let mut work = IoWork::default();
    let mut arrivals = completed.drain(..);
    let mut bin = None;
    loop {
        let mut cells = shared.lock();
        let bin = &*match &mut bin {
            Some(bin) => bin,
            slot => slot.insert(cells.host_bin(here.me, Arc::clone(&raw))?),
        };
        let mut rest = true;
        for Completion {
            key,
            result,
            cancelled,
        } in arrivals.by_ref()
        {
            let op = CellRef::decode(key as i64);
            settle_arrival(
                &mut cells,
                here,
                ctx,
                bin,
                depth_offset,
                op,
                result,
                cancelled,
                &mut work,
                out,
            )?;
            if !work.submits.is_empty() {
                rest = false;
                break;
            }
        }
        drop(cells);
        if rest {
            return Ok(());
        }
        run_io(
            shared,
            here,
            ctx,
            Some(&mut *driver),
            depth_offset,
            &mut work,
            out,
        )?;
    }
}

/// 記録 `op` の driver の鍵。記録の番号そのもので、`complete` が完了の鍵から記録を引き戻す
/// (docs/spec/vm.md の「操作の記録」)。
fn op_key(op: CellRef) -> OpKey {
    op.encode() as OpKey
}

/// プールの仕事の受け付け。仕事`job`を記録のhostのdriverに出し、tokenを返す。`from`は仕事の出どころで、取り出しが
/// 確かめる。`me`は呼んだタスクで、取り消しの地点なら仕事を出さない。記録は語を持たない (docs/spec/vm.md の
/// 「操作の記録」)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_job<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    driver: Option<&mut (dyn IoDriver + 'static)>,
    worker: u32,
    depth_offset: usize,
    me: CellRef,
    from: JobSource,
    job: Op,
) -> Result<i64, CellFault> {
    let record = net_record(OpKind::Job { from, answer: None }, worker, None);
    submit(
        shared,
        here,
        ctx,
        driver,
        depth_offset,
        record,
        Some(job),
        Some(me),
    )
}

/// `take_job`が返す、終わったプールの仕事。`result`はプールが返した箱か、仕事を走らせられなかった誤りか、取り消しの
/// 地点で出さなかった誤りである。
pub(crate) struct JobTaken {
    pub(crate) from: JobSource,
    result: io::Result<Box<dyn Any + Send>>,
}

impl JobTaken {
    /// 箱を、仕事を受け付けた行が入れた型に戻す。仕事の誤りはそのまま返す。違う型はVMの誤りである。
    pub(crate) fn answer<T: 'static>(self) -> Result<io::Result<T>, CellFault> {
        match self.result {
            Ok(answer) => answer
                .downcast::<T>()
                .map(|r| Ok(*r))
                .map_err(|_| CellFault::Internal("an op completes with another kind of result")),
            Err(e) => Ok(Err(e)),
        }
    }
}

/// プールの仕事の取り出し。tokenを消費して記録を放す。取り出すのは受け付けたタスク (`worker`) だけである。
pub(crate) fn take_job<M: Mode>(
    shared: &Shared,
    ctx: &mut Ctx<M>,
    token: i64,
    worker: u32,
) -> Result<JobTaken, CellFault> {
    let taken = take_op(shared, ctx, token, worker)?;
    let OpKind::Job { from, answer } = taken.kind else {
        return Err(CellFault::Internal("an op is taken as another kind of op"));
    };
    let result = match (taken.result, answer) {
        (Ok(_), Some(answer)) => Ok(answer),
        (Ok(_), None) => return Err(CellFault::Internal("a job op has no answer")),
        (Err(e), _) => Err(e),
    };
    Ok(JobTaken { from, result })
}

/// `Std.Net` の操作の記録。buffer を持たない操作はどれも driver が記憶を使わないので、段階は `Transfer` から始める。
/// 持ち主と host は表に入れるときに決める。
fn net_record(kind: OpKind, worker: u32, stream: Option<Word>) -> OpRecord {
    OpRecord {
        owner: 0,
        host: 0,
        submitted: false,
        pending_op: None,
        arrival: None,
        kind,
        phase: OpPhase::Transfer,
        worker,
        buf: None,
        stream,
        made: None,
        waiter: None,
        timer: None,
        cancelling: false,
        timed_out: false,
    }
}

/// 呼ぶ側から移ってきた `String` の参照 `w` の中身を写して、参照を手放す。
pub(crate) fn take_string<M: Mode>(ctx: &mut Ctx<M>, w: Word) -> Result<String, CellFault> {
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
    let s = unsafe { &*ctx.heap().foreign_unique::<IoStream>(stream)? };
    let Origin::Peer(peer) = s.origin else {
        return Err(CellFault::Internal("a file stream has no peer"));
    };
    Ok(ctx.fresh_string(&peer.to_string()))
}

/// `Std.Stream.source`。借りた読み手を開いたパスを、引用符で囲んだ新しい文字列にする。包みが読みの誤りの文言に使う
/// (docs/spec/stdlib.md の「`Std.Stream`」)。
pub(crate) fn stream_source<M: Mode>(ctx: &mut Ctx<M>, stream: Word) -> Result<Word, CellFault> {
    // 読み手は呼ぶ側が持つ唯一の参照で、借りて読むだけである (`Reader` は `Lin`)
    let s = unsafe { &*ctx.heap().foreign_unique::<IoStream>(stream)? };
    let name = match &s.origin {
        Origin::File(path) => format!("`{path}`"),
        Origin::Child { program, pipe } => format!("{} `{program}`", pipe.what()),
        Origin::Peer(_) => return Err(CellFault::Internal("a socket stream has no path")),
    };
    Ok(ctx.fresh_string(&name))
}

/// `Std.Exec.stdin`、`stdout`、`stderr`。借りた子 `child` から、まだ取り出していない pipe の端を移し、縛っていないまま
/// `Stream.Writer` か `Stream.Reader` の新しい物体にする。端がなければ (2回目か、pipe にしなかった向き)、IO の失敗の
/// 文言を返す (docs/spec/stdlib.md の「`Std.Exec`」)。
pub(crate) fn exec_take<M: Mode>(
    ctx: &mut Ctx<M>,
    child: Word,
    pipe: ChildPipe,
) -> Result<Result<Word, String>, CellFault> {
    // 子は呼ぶ側が持つ唯一の参照で、借りて端だけを移す (`Child` は `Lin`)
    let c = unsafe { &mut *ctx.heap().foreign_unique::<ExecChild>(child)? };
    let (end, stage, what) = match pipe {
        ChildPipe::Stdin => (c.stdin.take(), c.stages.first(), "the standard input"),
        ChildPipe::Stdout => (c.stdout.take(), c.stages.last(), "the output"),
        ChildPipe::Stderr => (c.stderr.take(), c.stages.last(), "the errors"),
    };
    let program = stage
        .map(|s| Arc::clone(&s.program))
        .ok_or(CellFault::Internal("a child has no stages"))?;
    let Some(bind) = end else {
        return Ok(Err(format!(
            "cannot take {what} of `{program}`: not available"
        )));
    };
    let stream = IoStream {
        bind,
        origin: Origin::Child { program, pipe },
        rest: Vec::new(),
        start: 0,
    };
    // 確保するだけで、`Ctx` の持つ語には触れない。`Reader` と `Writer` は `Lin` である
    Ok(Ok(unsafe { ctx.heap_mut() }.alloc_foreign(stream, true)))
}

/// `Std.Exec.stage_count`。借りた子の段の数。
pub(crate) fn exec_stage_count<M: Mode>(ctx: &Ctx<M>, child: Word) -> Result<i64, CellFault> {
    // 子は呼ぶ側が持つ唯一の参照で、借りて読むだけである (`Child` は `Lin`)
    let n = unsafe {
        (*ctx.heap().foreign_unique::<ExecChild>(child)?)
            .stages
            .len()
    };
    i64::try_from(n).map_err(|_| CellFault::Internal("a child has too many stages"))
}

/// `Std.Exec.submit_wait`。子 `child` は呼ぶ側から移ってきた参照で、記録に移す。取り出していない標準入力の端を先に
/// 閉じ、段 `k` の終わりを持ち主 `here.me` の driver に `Op::Wait` で待たせる。持ち主と host を同じ worker にするのは、
/// 待つタスクは救済されず、持ち主が動かないためである。driver の待ちは回収しないので、知らせの後で `reap` が回収する。
/// 回収した段と、OS の子を持たない Host の段は、driver に出さずに終わった記録にする (docs/spec/vm.md の「操作の記録」)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_wait<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    driver: Option<&mut (dyn IoDriver + 'static)>,
    worker: u32,
    depth_offset: usize,
    k: i64,
    child: Word,
) -> Result<i64, CellFault> {
    if here.host != here.me {
        return Err(CellFault::Internal(
            "a wait is submitted on a worker that hosts no I/O",
        ));
    }
    let k = usize::try_from(k)
        .map_err(|_| CellFault::Internal("a wait names a stage the child does not have"))?;
    // 子は呼ぶ側から移ってきた唯一の参照である (`Child` は `Lin`)
    let c = unsafe { &mut *ctx.heap().foreign_unique::<ExecChild>(child)? };
    if let Some(stdin) = c.stdin.take() {
        stdin.close();
    }
    let program = Arc::clone(&c.stage(k)?.program);
    let raw = c.raw(k)?;
    let mut record = net_record(OpKind::Wait { program }, worker, Some(child));
    let op = match raw {
        Some(process) => Some(Op::Wait { process }),
        None => {
            record.phase = OpPhase::Done(Ok(0), false);
            None
        }
    };
    submit(shared, here, ctx, driver, depth_offset, record, op, None)
}

/// `Std.Exec.reap`。借りた子 `child` の段 `k` を回収し、終了コードを返す。`Op::Wait` の知らせの後なので待たずに済む。
/// OS の子を持たない Host の段は、ここで止まって待つ。回収した段はその終了コードを返す。回収できなければ、IO の失敗の
/// 誤りと文言を返す (docs/spec/stdlib.md の「`Std.Exec`」)。
pub(crate) fn exec_reap<M: Mode>(
    ctx: &Ctx<M>,
    k: i64,
    child: Word,
) -> Result<Result<i64, (io::Error, String)>, CellFault> {
    // 子は呼ぶ側が持つ唯一の参照で、借りて段だけを回収する (`Child` は `Lin`)
    let c = unsafe { &mut *ctx.heap().foreign_unique::<ExecChild>(child)? };
    let k = usize::try_from(k)
        .map_err(|_| CellFault::Internal("a wait names a stage the child does not have"))?;
    let stage = c.stages.get_mut(k).ok_or(CellFault::Internal(
        "a wait names a stage the child does not have",
    ))?;
    if let Some(mut process) = stage.process.take() {
        match process.wait() {
            Ok(code) => stage.code = Some(i64::from(code)),
            // 回収できなかった段は子に戻し、破棄処理が kill して待つ
            Err(e) => {
                let message = format!(
                    "cannot wait for `{}`: {}",
                    stage.program,
                    eml_rt::io_reason(&e)
                );
                stage.process = Some(process);
                return Ok(Err((e, message)));
            }
        }
    }
    stage
        .code
        .map(Ok)
        .ok_or(CellFault::Internal("a reaped stage has no exit code"))
}

/// 受け付けに移ってきたストリーム `stream` の記録が、誤りの文言の前に付ける語の元。
fn stream_prefix<M: Mode>(ctx: &Ctx<M>, stream: Word) -> Result<Prefix, CellFault> {
    // ストリームは呼ぶ側から移ってきた唯一の参照である (`TcpStream`、`Reader`、`Writer` は `Lin`)
    Ok(unsafe { (*ctx.heap().foreign_unique::<IoStream>(stream)?).prefix() })
}

/// ストリーム `stream` の driver のハンドル。ストリームは呼ぶ側から移ってきた参照か記録が持つ参照で、持ち主が返すか
/// 落とす。
pub(crate) fn stream_handle<M: Mode>(ctx: &Ctx<M>, stream: Word) -> Result<IoHandle, CellFault> {
    // ストリームは持ち主のただ1つの参照である (`TcpStream` は `Lin`。docs/spec/vm.md の「操作の記録」)
    unsafe {
        (*ctx.heap().foreign_unique::<IoStream>(stream)?)
            .bind
            .handle()
    }
}

/// ストリーム `stream` が、受けたまままだ縛っていないか。そのストリームの操作を受け付ける worker は、先に自分の driver を
/// 持つ (docs/spec/vm.md の「時計と driver」)。
pub(crate) fn stream_unbound<M: Mode>(ctx: &Ctx<M>, stream: Word) -> Result<bool, CellFault> {
    // ストリームは呼ぶ側が持つ唯一の参照で、借りて読むだけである (`TcpStream` は `Lin`)
    Ok(unsafe {
        matches!(
            (*ctx.heap().foreign_unique::<IoStream>(stream)?).bind,
            Bind::Unbound(_)
        )
    })
}

/// 受けたまま縛っていないストリーム `stream` を、持ち主 `here.me` の driver に取り込んで縛り、ハンドルを返す。縛った
/// ストリームはそのハンドルを返す。取り込みを driver が断れば、ソケットは閉じ、ストリームは以後どの操作も誤りで終える
/// (docs/spec/vm.md の「操作の記録」)。driver はストリームのハンドルを引く `submit_*` が受け取ったもので、取り込みは
/// 持ち主のスレッドで行う。
fn bind_stream<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    driver: Option<&mut (dyn IoDriver + 'static)>,
    stream: Word,
) -> Result<io::Result<IoHandle>, CellFault> {
    // ストリームは呼ぶ側から移ってきた唯一の参照である (`TcpStream` は `Lin`)
    let s = unsafe { &mut *ctx.heap().foreign_unique::<IoStream>(stream)? };
    match &s.bind {
        Bind::Bound { handle, .. } => return Ok(Ok(*handle)),
        Bind::Lost => return Ok(Err(lost_stream())),
        Bind::Unbound(_) => {}
    }
    // 受けを `Done::Accepted` で終える driver は本物の driver だけで、本物の時計の run ではどの worker も自分の I/O を
    // 受け付ける。仮想の時計のテストの driver は `Done::Stream` を返すので、縛っていないストリームはない
    if here.host != here.me {
        return Err(CellFault::Internal(
            "an unbound stream is used on a worker that hosts no I/O",
        ));
    }
    let driver = driver.ok_or(CellFault::Internal("a worker without a driver"))?;
    // bin は取り込む前に引く。取り込んだ後に失敗して、driver にハンドルを残さないためである
    let bin = shared.lock().host_bin(here.me, driver.bin())?;
    let Bind::Unbound(res) = std::mem::replace(&mut s.bind, Bind::Lost) else {
        unreachable!("the stream is unbound");
    };
    let handle = match driver.adopt(res) {
        Ok(handle) => handle,
        Err(e) => return Ok(Err(e)),
    };
    s.bind = Bind::Bound {
        host: here.me,
        handle,
        bin,
    };
    Ok(Ok(handle))
}

/// 取り込みに失敗したストリームの、2回目からの操作の誤り。
fn lost_stream() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotConnected,
        "the stream was closed when a driver failed to take it",
    )
}

/// 記録が持つリスナーかストリーム `w` を縛った worker。そのハンドルを引く操作は、その worker が host である。新しい
/// ハンドルを作る操作 (`listen` と `connect`) は物体を持たず、持ち主の I/O を受け付ける worker が host になる
/// (docs/spec/vm.md の「操作の記録」)。
fn bound_host<M: Mode>(ctx: &Ctx<M>, kind: &OpKind, w: Word) -> Result<Option<u32>, CellFault> {
    // どちらも記録が持つ唯一の参照である (`TcpListener` と `TcpStream` は `Lin`)
    Ok(unsafe {
        match kind {
            OpKind::Accept => (*ctx.heap().foreign_unique::<NetListener>(w)?).bind.host(),
            // 子の終わりの待ちはハンドルを引かず、持ち主の driver に出す
            OpKind::Wait { .. } => None,
            _ => (*ctx.heap().foreign_unique::<IoStream>(w)?).bind.host(),
        }
    })
}

/// 1回の読みに貸す空きのバイト数の上限で、広げたときに貸すバイト数 (docs/spec/vm.md の「操作の記録」)。
pub(crate) const READ_ROOM: usize = 65_536;

/// 読みに貸す空きの下限。これより少ない空きは空きなしとみなす。確保器が長さを語に切り上げた余り (0〜7 バイト) に、
/// 読む量を決めさせないためである (docs/spec/vm.md の「操作の記録」)。
const MIN_ROOM: usize = 8;

/// 空きの足りない長さ `len` の buffer を広げた後の容量。長さだけで決めるので、足し続ける読みの写しは合わせて線形に
/// 収まる (docs/spec/vm.md の「操作の記録」)。
fn grown_capacity(len: usize) -> usize {
    len + len.max(READ_ROOM)
}

/// `read_line` の1行のバイト数の上限 (docs/spec/stdlib.md の「`Std.Net`」)。
pub(crate) const LINE_LIMIT: usize = 1_048_576;

/// ストリームの buffer の残りに見つけた最初の行。
pub(crate) enum Found {
    /// 終わりの改行を含めた行のバイト数。
    Line(usize),
    /// 行の中身が `LINE_LIMIT` バイトを超えた。
    TooLong,
}

/// `pending` の `from` バイト目から後に改行があるか、`eof` なら、`pending` の最初の行の、終わりの改行を含めた長さを返す。
/// `eof` で改行がなければ、`pending` のすべてが行である (EOF の前の最後の行)。行の中身 (終わりの `\n` か `\r\n` を
/// 除いたバイト、EOF ではすべて) が `LINE_LIMIT` バイトを超えれば `TooLong` である。上限を中身の長さで決めるので、
/// 結果はバイトが読みにどう分かれて届いたかによらない (docs/spec/stdlib.md の「`Std.Net`」)。どちらでもなければ `None`
/// である。
pub(crate) fn find_line(pending: &[u8], from: usize, eof: bool) -> Option<Found> {
    // 改行のない周が大半なので、まず語ごとに比べる `contains` で探し、あるときだけ位置を求める
    let newline = if pending[from..].contains(&b'\n') {
        pending[from..].iter().position(|&b| b == b'\n')
    } else {
        None
    };
    let (content, n) = match newline {
        Some(i) => {
            let end = from + i;
            let cr = end > 0 && pending[end - 1] == b'\r';
            (end - usize::from(cr), end + 1)
        }
        None if eof => (pending.len(), pending.len()),
        None => return None,
    };
    Some(if content > LINE_LIMIT {
        Found::TooLong
    } else {
        Found::Line(n)
    })
}

/// 改行のまだ来ない行の中身の、今わかっている長さ。最後の `\r` は `\r\n` の始まりかもしれないので数えない。
pub(crate) fn open_line_len(pending: &[u8]) -> usize {
    pending.len() - usize::from(pending.last() == Some(&b'\r'))
}

/// 行の連鎖の読みが終わった後の次の段階。
enum LineStep {
    /// 行も EOF もまだなく、上限にも届いていない。このハンドルで、もう一度読む。
    Again(IoHandle),
    /// 切り出した行を写した新しい物体と、それが文字列であること (`take_line_text`)。
    Line(Word, bool),
    /// 取り消しか期限で止めた。
    Stopped,
    /// 行の中身が `LINE_LIMIT` バイトを超えた。
    TooLong,
    /// 読みが誤りで終わった。
    Failed,
}

/// ストリーム `stream` の buffer に空きがあれば貸す。`grow` か、ストリームがソケットでなければ、空きが足りないときに
/// 広げてから貸す。読めるようになるのを待つのはソケットだけだからである (docs/spec/vm.md の「操作の記録」)。返すのは
/// ハンドルと、`IoStream::lend_room` が返す貸す前の長さと貸した記憶である。記録の worker ID に切り替えた下で呼ぶ。
fn lend_line_room<M: Mode>(
    ctx: &mut Ctx<M>,
    stream: Word,
    grow: bool,
) -> Result<(IoHandle, Option<(usize, RawBuf)>), CellFault> {
    // ストリームは記録が持つ唯一の参照である (docs/spec/vm.md の「操作の記録」)
    let s = unsafe { &mut *ctx.heap().foreign_unique::<IoStream>(stream)? };
    let lent = s.lend_room(grow || !s.is_socket());
    Ok((s.bind.handle()?, lent))
}

/// ストリーム `stream` の残りの先頭 `n` バイトの行を新しい物体に1回だけ写し、返したことにする。UTF-8 なら文字列に、
/// UTF-8 でなければ `Bytes` に写し、文字列に写したかを添えて返す。UTF-8 でない行も `Std.Stream.read_line_bytes`
/// が読めるように残す (docs/spec/stdlib.md の「`Std.Stream`」)。
fn take_line_text<M: Mode>(
    ctx: &mut Ctx<M>,
    stream: Word,
    n: usize,
) -> Result<(Word, bool), CellFault> {
    // ストリームは呼ぶ側か記録が持つ唯一の参照である。行のバイトは `Vec` 自身の確保の中にあり、物体を作っても動かない
    let s = unsafe { &mut *ctx.heap().foreign_unique::<IoStream>(stream)? };
    let line = &s.pending()[..n];
    let made = match std::str::from_utf8(line) {
        Ok(text) => (ctx.fresh_string(text), true),
        Err(_) => {
            let empty = ctx.bytes_empty()?;
            // 新しい `Bytes` は一意で、写すバイトはヒープの物体と重ならない
            (unsafe { ctx.heap_mut().bytes_extend(empty, line)? }, false)
        }
    };
    s.consume(n);
    Ok(made)
}

/// 行の連鎖の読みの完了を、ストリーム `stream` の buffer に確定して次の段階を決める。`got` は読んだバイト数で、誤りなら
/// `None` である。`stopped` は記録に取り消しか期限の印があることである。記録の worker ID に切り替えた下で呼ぶ。
fn line_step<M: Mode>(
    ctx: &mut Ctx<M>,
    stream: Word,
    filled: usize,
    got: Option<usize>,
    stopped: bool,
) -> Result<LineStep, CellFault> {
    // ストリームは記録が持つ唯一の参照で、driver は完了で書き終えている (docs/spec/vm.md の「操作の記録」)
    let s = unsafe { &mut *ctx.heap().foreign_unique::<IoStream>(stream)? };
    unsafe { s.commit_room(filled, got.unwrap_or(0)) };
    let Some(n) = got else {
        return Ok(LineStep::Failed);
    };
    // 空きを貸すときに返したバイトを詰めたので、残りは buffer の先頭から始まる。新しく読んだバイトだけを探す。前の
    // バイトには改行がなかった
    match s.find_line(filled, n == 0) {
        Some(Found::Line(n)) => {
            let (line, text) = take_line_text(ctx, stream, n)?;
            return Ok(LineStep::Line(line, text));
        }
        Some(Found::TooLong) => return Ok(LineStep::TooLong),
        None => {}
    }
    Ok(if stopped {
        LineStep::Stopped
    } else if s.open_line_len() > LINE_LIMIT {
        LineStep::TooLong
    } else {
        LineStep::Again(s.bind.handle()?)
    })
}

/// 行の中身が `LINE_LIMIT` バイトを超えたときの誤り (docs/spec/stdlib.md の「`Std.Net`」)。
fn line_too_long() -> io::Error {
    io::Error::other(format!("a line is longer than {LINE_LIMIT} bytes"))
}

/// ストリーム `stream` の buffer に残りがあれば、その先頭の `READ_ROOM` バイトまでを `buf` の後ろに足して返したことに
/// し、足した後の buffer の語とバイト数を返す。残りがなければ `None` で、`buf` はそのままである。
fn serve_rest<M: Mode>(
    ctx: &mut Ctx<M>,
    buf: Word,
    stream: Word,
) -> Result<Option<(Word, usize)>, CellFault> {
    // ストリームは呼ぶ側から移ってきた唯一の参照である (`TcpStream` は `Lin`)
    let s = unsafe { &mut *ctx.heap().foreign_unique::<IoStream>(stream)? };
    let n = s.pending().len().min(READ_ROOM);
    if n == 0 {
        return Ok(None);
    }
    // 写すバイトは `Vec` 自身の確保の中にあり、ヒープの物体と重ならない。`buf` が一意でなければ `bytes_extend` が写す
    let buf = unsafe { ctx.heap_mut().bytes_extend(buf, &s.pending()[..n])? };
    s.consume(n);
    Ok(Some((buf, n)))
}

/// 読みの受け付けに移ってきた `Bytes` の参照 `buf` が一意で、長さの後ろの空きが `MIN_ROOM` バイト以上あれば、そのうち
/// `READ_ROOM` バイトまでを読みに貸す。ほかは `None` で、何も変えない (docs/spec/vm.md の「操作の記録」)。
fn spare_room<M: Mode>(ctx: &mut Ctx<M>, buf: Word) -> Result<Option<RawBuf>, CellFault> {
    // `buf` は呼ぶ側から移ってきた今の worker の参照で、この後に作る記録へ移す。`Ctx` の持つ語には触れない
    let heap = unsafe { ctx.heap_mut() };
    if !unsafe { heap.is_unique(buf) }? {
        return Ok(None);
    }
    // 受け付けは driver に出す前に記録を作って buffer を移す。記録が物体を生かし、一意に保ち、完了を受け取るまで
    // 書き換えないので、driver はこの空きに書ける (docs/spec/object-model.md の T10 の行)
    let raw = unsafe { heap.bytes_lend(buf)? };
    Ok((raw.cap >= MIN_ROOM).then(|| RawBuf {
        ptr: raw.ptr,
        cap: raw.cap.min(READ_ROOM),
    }))
}

/// 読めるようになった端 `stream` の読みのために、記録の buffer `buf` を容量 `grown_capacity(長さ)` 以上の新しい物体に
/// 写して元を手放し、新しい物体の空きをちょうど `READ_ROOM` バイト貸す。`buf` は一意でないか空きが `MIN_ROOM` バイト
/// より少ない (docs/spec/vm.md の「操作の記録」)。返すのは記録が持つ buffer の語、端のハンドル、貸した記憶である。
/// 記録の worker ID に切り替えた下で呼ぶ。`HeapError` は検査モードの前提の違反で、driver に出す前に返すので、貸した
/// ままの記憶は残らない。確保量の上限は誤りにせず、次の安全点で止まる。
fn grow_read_room<M: Mode>(
    ctx: &mut Ctx<M>,
    buf: Word,
    stream: Word,
) -> Result<(Word, IoHandle, RawBuf), CellFault> {
    let handle = stream_handle(ctx, stream)?;
    let heap = unsafe { ctx.heap_mut() };
    let content = unsafe { heap.bytes(buf)? };
    let len = content.len();
    let mut fresh = heap.alloc_bytes_room(grown_capacity(len));
    if len > 0 {
        // `content` は `fresh` と別の物体の中身で、`fresh` を作っても動かない
        fresh = unsafe { heap.bytes_extend(fresh, &*content)? };
    }
    unsafe { heap.decref(buf)? };
    // `spare_room` と同じく、記録が新しい物体を一意に保つ (docs/spec/object-model.md の T10 の行)
    let raw = unsafe { heap.bytes_lend(fresh)? };
    let raw = RawBuf {
        ptr: raw.ptr,
        cap: READ_ROOM,
    };
    Ok((fresh, handle, raw))
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

    /// 容量 `cap` の channel を作る。容量は `0..=u32::MAX` に収める。預かり先 `depot` は、呼ぶ側が lock の外で
    /// `Depot::open` で開いたものである。
    pub(crate) fn new_chan(&mut self, cap: i64, depot: Depot) -> i64 {
        let cap = cap.clamp(0, i64::from(u32::MAX)) as u32;
        self.alloc(CellKind::Chan(ChanCell {
            cap,
            closed: false,
            items: VecDeque::new(),
            sends: 0,
            recvs: 0,
            senders: VecDeque::new(),
            receivers: VecDeque::new(),
            depot,
        }))
        .encode()
    }

    /// タスクの中なら予算を1つ使い、`yields` が真で使い切ったら真を返す。タスクの外では予算を使わない。
    fn spend(&mut self, me: Option<CellRef>, yields: bool) -> Result<bool, CellFault> {
        match me {
            Some(me) => Ok(self.spend_budget(me, 1)? && yields),
            None => Ok(false),
        }
    }

    /// 送りの速い道。0 なら枠を1つ予約して `hold` に置き、続く `push_raw` が確定する。1 なら待ち、2 なら閉じていて、
    /// 3 なら先に譲る。
    pub(crate) fn reserve_send(
        &mut self,
        c: i64,
        me: Option<CellRef>,
        yields: bool,
        hold: &mut Option<Hold>,
    ) -> Result<i64, CellFault> {
        if self.spend(me, yields)? {
            return Ok(3);
        }
        let chan = CellRef::decode(c);
        Ok(match self.chan_mut(chan) {
            None => 2,
            Some(cell) if cell.closed => 2,
            Some(cell) if cell.can_send() => {
                debug_assert!(hold.is_none(), "a channel reservation is made over another");
                cell.sends += 1;
                *hold = Some(Hold::Send {
                    chan,
                    depot: cell.depot.worker,
                });
                0
            }
            Some(_) => 1,
        })
    }

    /// 受け取りの速い道。0 なら先頭の品物を列から取って `hold` に置き、続く `pop_raw` が確定する。積めるようになれば
    /// 待っている送り手の先頭を1つ起こして `out` に積む。1 なら待ち、2 なら閉じて空で、3 なら先に譲る。
    pub(crate) fn reserve_recv(
        &mut self,
        c: i64,
        me: Option<CellRef>,
        yields: bool,
        hold: &mut Option<Hold>,
        out: &mut Vec<Woken>,
    ) -> Result<i64, CellFault> {
        if self.spend(me, yields)? {
            return Ok(3);
        }
        let chan = CellRef::decode(c);
        let Some(cell) = self.chan_mut(chan) else {
            return Ok(2);
        };
        let Some(item) = cell.items.pop_front() else {
            return Ok(if cell.drained() { 2 } else { 1 });
        };
        debug_assert!(hold.is_none(), "a channel reservation is made over another");
        cell.recvs += 1;
        *hold = Some(Hold::Recv {
            chan,
            depot: cell.depot.worker,
            item,
        });
        let sender = if cell.can_send() {
            cell.senders.pop_front()
        } else {
            None
        };
        if let Some(s) = sender {
            out.push(self.wake(s, chan)?);
        }
        Ok(0)
    }

    /// 送りの予約を確定する。品物 `x` は呼ぶ側が lock の外で預けたもので、列の後ろに積む。閉じていて予約がなくなれば
    /// 待っている受け手をすべて、そうでなければ先頭を1つ起こして `out` に積む。
    pub(crate) fn push_raw(
        &mut self,
        chan: CellRef,
        x: Word,
        out: &mut Vec<Woken>,
    ) -> Result<(), CellFault> {
        let cell = self
            .chan_mut(chan)
            .ok_or(CellFault::Internal("an item is pushed to a freed channel"))?;
        cell.sends = cell.sends.checked_sub(1).ok_or(CellFault::Internal(
            "an item is pushed without a reserved send",
        ))?;
        cell.items.push_back(x);
        let all = cell.drained();
        self.stats.channel_ops += 1;
        while let Some(r) = self
            .chan_mut(chan)
            .and_then(|cell| cell.receivers.pop_front())
        {
            out.push(self.wake(r, chan)?);
            if !all {
                break;
            }
        }
        Ok(())
    }

    /// 受け取りの予約を確定する。品物は呼ぶ側が lock の外で受け取った。放した channel の預かり先を返すので、呼ぶ側は
    /// lock を放してから閉じる。
    pub(crate) fn pop_raw(&mut self, chan: CellRef) -> Result<Option<Depot>, CellFault> {
        let cell = self.chan_mut(chan).ok_or(CellFault::Internal(
            "an item is popped from a freed channel",
        ))?;
        cell.recvs = cell.recvs.checked_sub(1).ok_or(CellFault::Internal(
            "an item is popped without a reserved receive",
        ))?;
        self.stats.channel_ops += 1;
        self.settle(chan)
    }

    /// 閉じる。待っている受け手と送り手をすべて起こして `out` に積み、放せるなら放して預かり先を返す。放したか閉じた
    /// channel には何もしない。送りの予約がなければ、同じ channel で受け手と送り手が同時に待つことはないので、起こす順は
    /// 列ごとの FIFO だけで決まる。
    pub(crate) fn close_raw(
        &mut self,
        c: i64,
        out: &mut Vec<Woken>,
    ) -> Result<Option<Depot>, CellFault> {
        let chan = CellRef::decode(c);
        match self.chan_mut(chan) {
            Some(cell) if !cell.closed => cell.closed = true,
            _ => return Ok(None),
        }
        while let Some(t) = self
            .chan_mut(chan)
            .and_then(|cell| cell.receivers.pop_front())
        {
            out.push(self.wake(t, chan)?);
        }
        while let Some(t) = self
            .chan_mut(chan)
            .and_then(|cell| cell.senders.pop_front())
        {
            out.push(self.wake(t, chan)?);
        }
        self.settle(chan)
    }

    /// channel の待ち手 `t` を起こす。列から外した後に呼ぶ。枠も品物も取っておかず、起こされたタスクが速い道を
    /// やり直す。
    fn wake(&mut self, t: CellRef, chan: CellRef) -> Result<Woken, CellFault> {
        let task = self.task_mut(t)?;
        if !matches!(task.phase, Phase::Parked)
            || !matches!(task.wait, Some(Wait::Cell(c)) if c == chan)
        {
            return Err(CellFault::Internal("a channel waiter is not waiting"));
        }
        self.wake_cell(t, WOKE, None)?
            .ok_or(CellFault::Internal("a channel waiter is not waiting"))
    }

    /// タスク `me` が、継続 `k` で channel `c` に枠が空くか閉じるまで待つ。
    pub(crate) fn park_send(
        &mut self,
        c: i64,
        me: CellRef,
        k: Word,
        out: &mut Vec<Woken>,
    ) -> Result<Option<Runnable>, CellFault> {
        self.park_chan(c, me, k, Side::Send, out)
    }

    /// タスク `me` が、継続 `k` で channel `c` に品物が来るか、閉じて送りの予約がなくなるまで待つ。
    pub(crate) fn park_recv(
        &mut self,
        c: i64,
        me: CellRef,
        k: Word,
        out: &mut Vec<Woken>,
    ) -> Result<Option<Runnable>, CellFault> {
        self.park_chan(c, me, k, Side::Recv, out)
    }

    /// `park` と同じく、止めて数え、取り消しの規則を確かめる。すぐ戻れるなら `Woke` で戻る仕事を返し、そうでなければ
    /// 待ち手の列に登録する。受け手の登録は枠を1つ開くので、積めるようになれば送り手の先頭を1つ起こして `out` に積む。
    fn park_chan(
        &mut self,
        c: i64,
        me: CellRef,
        k: Word,
        side: Side,
        out: &mut Vec<Woken>,
    ) -> Result<Option<Runnable>, CellFault> {
        let cell = CellRef::decode(c);
        let worker = self.suspend(me)?;
        self.stats.parks += 1;
        self.stats.channel_waits += 1;
        if self.point(me)? {
            self.stats.cancels += 1;
            return Ok(Some(Runnable::Resume {
                cell: me,
                worker,
                k,
                wake: CANCELLED,
            }));
        }
        // 生きた channel でない値は、閉じて空の channel として扱う。ほかの worker が速い道と待ちの間に channel を
        // 変えうるので、ここで確かめ直す。1つの worker では速い道が確かめた状態のまま待ちに来るので届かない
        let ready = match self.chan_mut(cell) {
            None => true,
            Some(chan) => match side {
                Side::Send => chan.closed || chan.can_send(),
                Side::Recv => !chan.items.is_empty() || chan.drained(),
            },
        };
        if ready {
            return Ok(Some(Runnable::Resume {
                cell: me,
                worker,
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
        if let Some(s) = sender {
            out.push(self.wake(s, cell)?);
        }
        Ok(None)
    }

    /// 閉じて空で予約のない channel を放し、預かり先を返す。品物は残っていないので、呼ぶ側は lock を放してから預かる
    /// ID を退かせるだけである。予約はどれも、その `publish` か `receive` の後に lock の下で下ろすので、退かせるのは
    /// それらすべての後になる (docs/spec/object-model.md の「論理的な worker ID」)。
    fn settle(&mut self, cell: CellRef) -> Result<Option<Depot>, CellFault> {
        if self.chan_mut(cell).is_some_and(|chan| chan.released()) {
            return Ok(Some(self.take_chan(cell)?.depot));
        }
        Ok(None)
    }

    /// run の終わりに残った channel を放す。残った品物を預かる ID の下で落としてから ID を退かせることを `discards` に
    /// 積む。`sweep_all` は成功した run の終わりにだけ呼び、予約と確定の間に切り替えはないので、予約は残っていない。
    fn free_chan(&mut self, cell: CellRef, discards: &mut Discards) -> Result<(), CellFault> {
        let ChanCell {
            items,
            depot,
            sends,
            recvs,
            ..
        } = self.take_chan(cell)?;
        debug_assert!(
            sends == 0 && recvs == 0,
            "a channel is swept with a reservation"
        );
        for item in items {
            discards.word(item, depot.worker);
        }
        discards.retire(depot.worker);
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

/// 借りたハンドル `handle` のセル。ハンドルの持つ `Shared` が `shared` と違えば、前の run のハンドルである。ハンドルは
/// 落ちたときに mail に積むために自分の run の `Shared` を持つので、それを比べれば run を見分けられる。
/// `Task a` は `Lin` で入力をまたいで残らないので、検査を通ったプログラムからは届かない
/// (docs/spec/vm.md の「`Async` のタスク」)。ハンドルは呼ぶ側の物体なので、登録表の lock の外で読む。
pub(crate) fn handle_cell<M: Mode>(
    ctx: &Ctx<M>,
    shared: &Arc<Shared>,
    handle: Word,
) -> Result<CellRef, CellFault> {
    // ハンドルは `Task a` の `Foreign` で、呼ぶ側が持つ唯一の参照である (`Task a` は `Lin`)
    let h = unsafe { ctx.heap().foreign_unique::<TaskHandle>(handle) }?;
    let h = unsafe { &*h };
    if !Arc::ptr_eq(&h.shared, shared) {
        return Err(CellFault::Internal("a task handle outlived its run"));
    }
    Ok(h.cell)
}

/// 借りたハンドル `handle` のセルの番号を `Std.Async.Cell` の即値で返す。
pub(crate) fn cell_of<M: Mode>(
    ctx: &Ctx<M>,
    shared: &Arc<Shared>,
    handle: Word,
) -> Result<i64, CellFault> {
    Ok(handle_cell(ctx, shared, handle)?.encode())
}

/// `start` が置いたタスク `cell` のハンドルを、登録表の lock を放してから今の worker の物体として作る。
pub(crate) fn task_handle<M: Mode>(ctx: &mut Ctx<M>, shared: &Arc<Shared>, cell: CellRef) -> Word {
    // 確保するだけで、`Ctx` の持つ語には触れない。`Task a` は `Lin` である
    unsafe { ctx.heap_mut() }.alloc_foreign(
        TaskHandle {
            cell,
            shared: Arc::clone(shared),
            taken: false,
        },
        true,
    )
}

/// ハンドルに受け取った印を付けて手放す。印があるので破棄処理は mail に積まない。
fn give_up<M: Mode>(ctx: &mut Ctx<M>, handle: Word) -> Result<(), CellFault> {
    unsafe {
        (*ctx.heap().foreign_unique::<TaskHandle>(handle)?).taken = true;
        ctx.heap_mut().decref(handle)?;
    }
    Ok(())
}

/// ハンドル `handle` を消費して、終わったタスクの値を返すか、失敗を返す。`handle` は呼ぶ側が持つ参照である。
/// 値は lock を放してから、終わったタスクの ID から今の worker へ戻し、その後でタスクの ID を退かせる。
pub(crate) fn take<M: Mode>(
    shared: &Arc<Shared>,
    ctx: &mut Ctx<M>,
    handle: Word,
    kind: Take,
    depth_offset: usize,
) -> Result<Result<Word, Raise>, CellFault> {
    let cell = handle_cell(ctx, shared, handle)?;
    let (mut dropped, mut retired) = (Discards::default(), Discards::default());
    let taken = shared.lock().take(cell, kind, &mut dropped, &mut retired)?;
    let result = match taken {
        Ok((w, worker)) => Ok(Depot::of(worker).receive(ctx, w)?),
        Err(raise) => {
            dropped.run(ctx, depth_offset)?;
            Err(raise)
        }
    };
    // ハンドルの参照は呼ぶ側から移ってきたもので、ここで手放す。受け取った印を先に付けるので、破棄処理は mail に
    // 積まない。タスクの ID は、その ID の値を受け取るか落とした後で退かせる
    give_up(ctx, handle)?;
    retired.run(ctx, depth_offset)?;
    Ok(result)
}

/// `detach` と `cancel_raw`。ハンドルを手放し、終わったタスクのセルを放すか、持ち手のないタスクにする。
pub(crate) fn detach<M: Mode>(
    shared: &Arc<Shared>,
    here: &Here,
    ctx: &mut Ctx<M>,
    handle: Word,
    cancel: bool,
    depth_offset: usize,
) -> Result<(), CellFault> {
    let cell = handle_cell(ctx, shared, handle)?;
    give_up(ctx, handle)?;
    let mut discards = Discards::default();
    shared.lock().let_go(here, cell, cancel, &mut discards)?;
    discards.run(ctx, depth_offset)
}

/// タスク `cell` を `outcome` で終え、worker `here.me` で走らせる起こしを `out` に積み、ほかの worker の起こしを送る。
/// ハンドルがもう落ちていれば、lock を放してから値を落とし、タスクの ID を退かせる。`depth_offset` は呼んだ実行系の
/// タスクの深さの起点である。
pub(crate) fn finish<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    cell: CellRef,
    outcome: Outcome,
    depth_offset: usize,
    out: &mut Vec<Runnable>,
) -> Result<(), CellFault> {
    let mut discards = Discards::default();
    {
        let mut cells = shared.lock();
        let mut woken = Vec::new();
        cells.finish(here, cell, outcome, &mut woken, &mut discards)?;
        cells.route(&mut woken, here, out)?;
    }
    discards.run(ctx, depth_offset)
}

/// 仕事 `cell` を `end` で終え、待っていたタスクの起こしを worker `here.me` で走らせる仕事にして `out` に積むか、
/// ほかの worker の持ち主へ送る。語には触れないので、lock の下では `Ctx` もヒープも呼ばない。
pub(crate) fn finish_job(
    shared: &Shared,
    here: &Here,
    cell: CellRef,
    end: JobEnd,
    out: &mut Vec<Runnable>,
) -> Result<(), CellFault> {
    let mut cells = shared.lock();
    let mut woken = Vec::new();
    cells.finish_job(cell, end, &mut woken)?;
    cells.route(&mut woken, here, out)
}

/// mail の項を処理し、取り消しで待ちを外したタスクを worker `here.me` で再開する仕事を `out` に、旗を付ける VM の
/// `Task` がこの worker にあるタスクのセルを `limited` に積む。どの worker も呼んでよく、落ちたハンドル、シグナル、
/// 上限の旗の項は最初に集めた worker が、取り消しの項はその持ち主が処理する。項は葉の
/// lock の下でまとめて取り出してから処理し、処理が新しい項を積むので、取り出す項がなくなるまで繰り返す。項ごとに短い lock の区間で処理し、lock を放してから、その項の driver への
/// 取り消し、語の破棄と ID の退かせ、シグナルを保留から下ろすことを行う。破棄の破棄処理が積む項は、それより後に積まれた
/// 項より先に処理する (docs/spec/vm.md の「セルと登録表」)。項を1つでも処理したら真を返す。
pub(crate) fn drain<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    mut driver: Option<&mut (dyn IoDriver + 'static)>,
    depth_offset: usize,
    out: &mut Vec<Runnable>,
    limited: &mut Vec<CellRef>,
) -> Result<bool, CellFault> {
    let (mut work, mut discards, mut woken) = (IoWork::default(), Discards::default(), Vec::new());
    let mut drained = false;
    loop {
        let items = shared.mail.take(here.me);
        if items.is_empty() {
            return Ok(drained);
        }
        drained = true;
        for item in items {
            let settled = {
                let mut cells = shared.lock();
                let bits = cells.receive_mail(
                    here,
                    item,
                    &mut woken,
                    &mut work,
                    &mut discards,
                    limited,
                )?;
                cells.route(&mut woken, here, out)?;
                bits
            };
            work.cancel_on(driver.as_deref_mut())?;
            discards.run(ctx, depth_offset)?;
            // 届けたので保留から下ろし、次の同じシグナルをハンドラが1回目として扱うようにする
            // (docs/spec/vm.md の「上限と割り込み」)
            if settled != 0 {
                ctx.heap().control().settle(settled);
            }
        }
    }
}

/// worker `me` の区画で期限が `now_ms` までのタイマーを外し、再開する仕事を `out` に積む。操作の待ちの期限の取り消しは
/// lock を放してから driver に頼む。集める地点はこの区間を見張りの語の書き込みとまとめて自分で行う。
#[cfg(test)]
fn expire(
    shared: &Shared,
    driver: &mut dyn IoDriver,
    me: u32,
    now_ms: u64,
    out: &mut Vec<Runnable>,
) -> Result<(), CellFault> {
    let mut work = IoWork::default();
    {
        let mut cells = shared.lock();
        let mut woken = Vec::new();
        let here = Here {
            me,
            host: me,
            mail: &shared.mail,
            post: &Unposted,
        };
        cells.expire(now_ms, me as usize, &here, &mut woken, &mut work)?;
        cells.route(&mut woken, &here, out)?;
    }
    work.cancel_all(driver);
    Ok(())
}

/// ほかの worker へ何も送らないはずのテストの宛先。送れば失敗にする。
#[cfg(test)]
struct Unposted;

#[cfg(test)]
impl Post for Unposted {
    fn wake(&self, k: u32, _w: Woken) {
        panic!("a wake is sent to worker {k}");
    }

    fn cancel(&self, k: u32, _cell: CellRef) {
        panic!("a cancel is sent to worker {k}");
    }

    fn place(&self, k: u32, _cell: CellRef, _worker: u32, _closure: Word) {
        panic!("a task is placed on worker {k}");
    }

    fn submit(&self, k: u32, _op: CellRef) {
        panic!("an op is submitted to worker {k}");
    }

    fn cancel_io(&self, k: u32, _op: CellRef) {
        panic!("an op cancel is sent to worker {k}");
    }

    fn completion(&self, k: u32, _op: CellRef) {
        panic!("a completion is sent to worker {k}");
    }

    fn limit(&self, k: u32, _cell: CellRef) {
        panic!("a limit is sent to worker {k}");
    }
}

/// worker 0 だけの run の、ほかの worker へ何も送らない `Here`。driver の操作は worker 0 が受け付ける。
#[cfg(test)]
pub(crate) fn solo(shared: &Shared) -> Here<'_> {
    Here {
        me: 0,
        host: 0,
        mail: &shared.mail,
        post: &Unposted,
    }
}

/// 持ち主 `here.me` のタスク `me` が token `token` の操作を待つ。1つの lock の区間で、host が置いた結果があれば先に
/// 当ててから `Registry::park_io` を行い、lock を放してから自分の driver への取り消しを頼む。ほかの worker が host の
/// 操作の取り消しと出し直しは、その区間で host へ送る (docs/spec/vm.md の「操作の記録」)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn park_io<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    driver: Option<&mut (dyn IoDriver + 'static)>,
    depth_offset: usize,
    token: i64,
    me: CellRef,
    k: Word,
) -> Result<Option<Runnable>, CellFault> {
    let mut work = IoWork::default();
    let run = {
        let mut cells = shared.lock();
        cells.settle_token(ctx, depth_offset, token, me, here, &mut work)?;
        cells.park_io(here, &mut work, token, me, k)?
    };
    work.cancel_on(driver)?;
    Ok(run)
}

/// `park_io` に期限を付けた待ち。区間と取り消しの扱いは `park_io` と同じである。
#[allow(clippy::too_many_arguments)]
pub(crate) fn park_io_until<M: Mode>(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<M>,
    driver: Option<&mut (dyn IoDriver + 'static)>,
    depth_offset: usize,
    now_ms: u64,
    ms: i64,
    token: i64,
    me: CellRef,
    k: Word,
) -> Result<Option<Runnable>, CellFault> {
    let mut work = IoWork::default();
    let run = {
        let mut cells = shared.lock();
        cells.settle_token(ctx, depth_offset, token, me, here, &mut work)?;
        cells.park_io_until(here, &mut work, now_ms, ms, token, me, k)?
    };
    work.cancel_on(driver)?;
    Ok(run)
}

/// run の終わりに、残ったセルをすべて放す。lock を放してから語を落として ID を退かせ、その間に積まれた mail の項は
/// 捨てる (docs/spec/vm.md の「終わり」)。
pub(crate) fn sweep_all<M: Mode>(
    shared: &Shared,
    ctx: &mut Ctx<M>,
    depth_offset: usize,
) -> Result<(), CellFault> {
    let mut discards = Discards::default();
    shared.lock().sweep_all(&mut discards)?;
    discards.run(ctx, depth_offset)?;
    drop(shared.mail.take_all());
    Ok(())
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
    /// 新しい預かる ID を出す。`new_raw` の行が、`new_chan` の前に登録表の lock の外で1回呼ぶ。
    pub(crate) fn open<M: Mode>(ctx: &mut Ctx<M>) -> Depot {
        Depot {
            worker: unsafe { ctx.heap_mut() }.fork_worker(),
        }
    }

    /// ほかで出した ID (タスクのセルの ID と、channel の予約が覚えた ID) を預かる ID として使う。`close` は呼ぶ側が ID を退かせる所で呼ぶ。
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
/// 深さの起点 `depth_offset`、深さの上限に戻す。RC には触れない (docs/spec/object-model.md の「タスクの切り替え」)。
pub(crate) fn with_worker<M: Mode, T>(
    ctx: &mut Ctx<M>,
    id: u32,
    depth_offset: usize,
    f: impl FnOnce(&mut Ctx<M>) -> T,
) -> T {
    let prev = ctx.heap().worker();
    // 実行系はタスクの上限の状態 (保留と猶予) を深さの上限の値で表すので、`switch_task` が書き直した値を元に戻す
    // (docs/spec/vm.md の「タスクの上限」)
    let limit = ctx.depth_limit();
    // 入れる `()` は即値で、取り出した `evv` は戻すときにそのまま返すので、参照の数は変わらない
    let evv = unsafe { ctx.switch_task(Word::UNIT, id, 0) };
    let result = f(ctx);
    let _unit = unsafe { ctx.switch_task(evv, prev, depth_offset) };
    ctx.set_depth_limit(limit);
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
fn running_task(registry: &Shared, scope: CellRef) -> CellRef {
    registry.lock().alloc(CellKind::Task(TaskCell {
        owner: 0,
        entries: 0,
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
        fast_yield: false,
        woke_from_job: false,
        run: None,
        detached: false,
        limit: false,
        bytes: 0,
    }))
}

/// 取り消しの項を受けたタスクの待ちを、集める地点と同じく lock の下で外し、lock を放してから driver に取り消しを頼む。
#[cfg(test)]
fn unpark(registry: &Shared, driver: &mut dyn IoDriver, cell: CellRef, out: &mut Vec<Runnable>) {
    let mut work = IoWork::default();
    {
        let mut cells = registry.lock();
        let mut woken = Vec::new();
        let here = solo(registry);
        cells.unpark(cell, &here, &mut woken, &mut work).unwrap();
        cells.route(&mut woken, &here, out).unwrap();
    }
    work.cancel_all(driver);
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
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let first = running_task(&registry, scope);
    let second = running_task(&registry, scope);
    assert!(
        registry
            .lock()
            .park_time(3, 50, first, Word::UNIT)
            .unwrap()
            .is_none()
    );
    assert!(
        registry
            .lock()
            .park_time(3, 50, second, Word::UNIT)
            .unwrap()
            .is_none()
    );
    assert_eq!(registry.lock().next_deadline(0), Some(53));
    let mut out = Vec::new();
    expire(&registry, &mut NoDriver::default(), 0, 52, &mut out).unwrap();
    assert!(out.is_empty());
    expire(&registry, &mut NoDriver::default(), 0, 53, &mut out).unwrap();
    assert_eq!(resumed_cells(&out), [(first, WOKE), (second, WOKE)]);
    assert_eq!(registry.lock().next_deadline(0), None);
}

#[test]
fn unparking_a_sleeping_task_removes_its_timer() {
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let first = running_task(&registry, scope);
    let second = running_task(&registry, scope);
    registry.lock().park_time(0, 50, first, Word::UNIT).unwrap();
    registry
        .lock()
        .park_time(0, 50, second, Word::UNIT)
        .unwrap();
    registry.lock().task_mut(first).unwrap().cancel = true;
    let mut out = Vec::new();
    unpark(&registry, &mut NoDriver::default(), first, &mut out);
    assert_eq!(resumed_cells(&out), [(first, CANCELLED)]);
    out.clear();
    expire(&registry, &mut NoDriver::default(), 0, 50, &mut out).unwrap();
    assert_eq!(resumed_cells(&out), [(second, WOKE)]);
}

/// 仕事の待ちは取り消しで外れない。待つタスクは継続だけになって登録表にあり、取り消しは仕事が終わってからの再開で
/// 1回だけ数える (docs/spec/vm.md の「取り消しの届け方」)。
#[test]
fn a_cancel_leaves_a_task_parked_on_a_job() {
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let job = registry.lock().new_job(1, me);
    assert!(
        registry
            .lock()
            .park(job.encode(), me, Word::UNIT)
            .unwrap()
            .is_none()
    );
    {
        let mut cells = registry.lock();
        let task = cells.task_mut(me).unwrap();
        assert!(task.k.is_some());
        assert!(matches!(task.wait, Some(Wait::Job(c)) if c == job));
    }
    registry.lock().task_mut(me).unwrap().cancel = true;
    let mut woken = Vec::new();
    let mut work = IoWork::default();
    registry
        .lock()
        .cancel_task(me, &solo(&registry), &mut woken, &mut work)
        .unwrap();
    assert!(woken.is_empty());
    assert_eq!(registry.lock().stats.cancels, 0);
    let mut out = Vec::new();
    finish_job(
        &registry,
        &solo(&registry),
        job,
        JobEnd::Value(Word::UNIT),
        &mut out,
    )
    .unwrap();
    assert_eq!(resumed_cells(&out), [(me, WOKE)]);
    assert_eq!(
        registry.lock().resumed(me, WOKE, false).unwrap().0,
        CANCELLED
    );
    assert_eq!(registry.lock().stats.cancels, 1);
    let (worker, end) = registry.lock().take_job(job.encode(), me).unwrap();
    assert_eq!(worker, 1);
    assert!(matches!(end, JobEnd::Value(_)));
}

/// 終わった仕事への park は待ち手を登録せず、すぐ再開の項を返す。待ちは数える。
#[test]
fn a_task_parked_on_a_finished_job_resumes_at_once() {
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let job = registry.lock().new_job(1, me);
    let mut out = Vec::new();
    finish_job(
        &registry,
        &solo(&registry),
        job,
        JobEnd::Value(Word::UNIT),
        &mut out,
    )
    .unwrap();
    assert!(out.is_empty());
    let run = registry.lock().park(job.encode(), me, Word::UNIT).unwrap();
    assert!(matches!(
        run,
        Some(Runnable::Resume { cell, wake: WOKE, .. }) if cell == me
    ));
    assert_eq!(registry.lock().stats.parks, 1);
    assert!(registry.lock().job_mut(job).unwrap().waiter.is_none());
    assert_eq!(registry.lock().resumed(me, WOKE, false).unwrap().0, WOKE);
    registry.lock().take_job(job.encode(), me).unwrap();
}

/// 仕事を待てるのも取り出せるのも、出したタスクだけである (docs/spec/vm.md の「セルと登録表」)。
#[test]
fn a_job_of_another_task_is_an_internal_error() {
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let other = running_task(&registry, scope);
    let job = registry.lock().new_job(1, me);
    assert!(matches!(
        registry.lock().park(job.encode(), other, Word::UNIT),
        Err(CellFault::Internal(
            "a task waits for the job of another task"
        ))
    ));
    assert!(matches!(
        registry.lock().take_job(job.encode(), other),
        Err(CellFault::Internal("a task takes the job of another task"))
    ));
    assert!(matches!(
        registry.lock().take_job(job.encode(), me),
        Err(CellFault::Internal("a job is taken before it ends"))
    ));
}

#[test]
fn a_slot_retires_instead_of_wrapping_its_generation() {
    let mut registry = Registry::new(vec![RETIRED - 2], 1);
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
    let mut later = Registry::new(registry.next_generations(), 1);
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
/// ものである。読みに貸された記憶を覚え、テストが外の書き手としてそこに書く。`finish_writes` が立っていれば、書きを
/// 受け付けの時点で終える。書きに貸された記憶は、本物の driver と同じく、書きを終える時点 (受け付けか、その鍵の完了を
/// 返すとき) に読んで `written` に写す。
#[cfg(test)]
#[derive(Default)]
struct NoDriver {
    /// 最後に受け付けた鍵と、受け付けた数。
    last_key: OpKey,
    submits: usize,
    cancelled: Vec<OpKey>,
    done: Vec<Completion>,
    lent: Vec<RawBuf>,
    finish_writes: bool,
    /// 書きに貸された記憶。受け付けの順に並ぶ。
    writes: Vec<RawBuf>,
    /// 保留にした書きの鍵と記憶。
    held: Vec<(OpKey, RawBuf)>,
    /// 書きを終えたときに貸された記憶から読んだ中身。
    written: Vec<Vec<u8>>,
    /// 取り込んだソケットの数。取り込んだソケットには 100 から順にハンドルを振る。
    adopted: u64,
    /// 立っていれば、取り込みを断る。
    refuse_adopt: bool,
}

/// 書きに貸された記憶の中身。貸した側は書きを終えるまで記憶を生かし、書き換えない (docs/spec/vm.md の「時計と driver」の
/// `RawBuf`)。
#[cfg(test)]
fn lent_content(buf: RawBuf) -> Vec<u8> {
    if buf.cap == 0 {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(buf.ptr, buf.cap) }.to_vec()
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

    fn submit(&mut self, key: OpKey, op: Op) -> Submit {
        if let Op::Read { buf, .. } = op {
            self.lent.push(buf);
        }
        self.last_key = key;
        self.submits += 1;
        match op {
            // 本物の driver と同じく、取り込みは受け付けで終える
            Op::Adopt { res } => Submit::Done(self.adopt(res).map(Done::Handle)),
            Op::Write { buf, .. } => {
                self.writes.push(buf);
                if self.finish_writes {
                    self.written.push(lent_content(buf));
                    Submit::Done(Ok(Done::Size(buf.cap)))
                } else {
                    self.held.push((key, buf));
                    Submit::Pending
                }
            }
            _ => Submit::Pending,
        }
    }

    fn cancel(&mut self, key: OpKey) {
        self.cancelled.push(key);
    }

    fn completions(&mut self, out: &mut Vec<Completion>) {
        for completion in &self.done {
            if let Some(at) = self.held.iter().position(|&(key, _)| key == completion.key) {
                let (_, buf) = self.held.remove(at);
                if completion.result.is_ok() {
                    self.written.push(lent_content(buf));
                }
            }
        }
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

    fn close(&mut self, _handle: IoHandle) {}

    fn adopt(&mut self, _res: RawResource) -> io::Result<IoHandle> {
        if self.refuse_adopt {
            return Err(io::Error::other("the driver refused the socket"));
        }
        self.adopted += 1;
        Ok(99 + self.adopted)
    }

    fn pipe(&mut self, _parent_reads: bool) -> io::Result<eml_rt::PipePair> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn quiesce(&mut self, _jobs: std::time::Duration) {}
}

/// 出力を捨てる Host の上の、検査モードの `Ctx`。
#[cfg(test)]
pub(crate) fn test_ctx() -> Ctx<eml_rt::Checked> {
    mode_ctx()
}

/// 出力を捨てる Host の上の、モード `M` の `Ctx`。
#[cfg(test)]
fn mode_ctx<M: Mode>() -> Ctx<M> {
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

/// 1つの worker の run で、タスク `me` が token `token` の操作を待つ。継続は即値である。
#[cfg(test)]
fn park_on<M: Mode>(
    registry: &Shared,
    ctx: &mut Ctx<M>,
    driver: &mut NoDriver,
    token: i64,
    me: CellRef,
) -> Result<Option<Runnable>, CellFault> {
    park_io(
        registry,
        &solo(registry),
        ctx,
        Some(driver),
        0,
        token,
        me,
        Word::UNIT,
    )
}

/// `park_on` に、今の時刻 `now_ms` から `ms` ミリ秒の期限を付けた待ち。
#[cfg(test)]
fn park_until_on<M: Mode>(
    registry: &Shared,
    ctx: &mut Ctx<M>,
    driver: &mut NoDriver,
    now_ms: u64,
    ms: i64,
    token: i64,
    me: CellRef,
) -> Result<Option<Runnable>, CellFault> {
    park_io_until(
        registry,
        &solo(registry),
        ctx,
        Some(driver),
        0,
        now_ms,
        ms,
        token,
        me,
        Word::UNIT,
    )
}

/// `b"x"` を書く操作を受け付け、token を返す。
#[cfg(test)]
fn submit_x(
    registry: &Shared,
    ctx: &mut Ctx<eml_rt::Checked>,
    driver: &mut NoDriver,
    worker: u32,
) -> i64 {
    submit_bytes(registry, ctx, driver, worker, b"x")
}

/// `bytes` を書く操作を出し、token を返す。
#[cfg(test)]
fn submit_bytes(
    registry: &Shared,
    ctx: &mut Ctx<eml_rt::Checked>,
    driver: &mut NoDriver,
    worker: u32,
    bytes: &[u8],
) -> i64 {
    let buf = unsafe { ctx.heap_mut() }.alloc_bytes(bytes);
    let stream = net_stream(ctx);
    submit_write(
        registry,
        &solo(registry),
        ctx,
        Some(&mut *driver),
        worker,
        0,
        buf,
        stream,
    )
    .unwrap()
}

/// 取り消しの地点で、もう終わった操作を待つと `Cancelled` で再開し、記録に取り消しの印を付ける。印がないと、取り出しの
/// 行が終わった操作の誤りを投げ、包みが `Cancelled` を投げられない (docs/spec/vm.md の「命令」の `park_io`)。
#[test]
fn a_cancelled_park_on_a_finished_op_marks_the_record_cancelling() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let token = submit_x(&registry, &mut ctx, &mut driver, 0);
    let op = CellRef::decode(token);
    registry.lock().parts[0].in_flight = 0;
    registry.lock().op_mut(op).unwrap().phase =
        OpPhase::Done(Err(io::Error::from(io::ErrorKind::BrokenPipe)), false);
    registry.lock().task_mut(me).unwrap().cancel = true;
    let run = park_on(&registry, &mut ctx, &mut driver, token, me).unwrap();
    assert!(matches!(
        run,
        Some(Runnable::ResumeIo { cell, wake: CANCELLED, .. }) if cell == me
    ));
    // 終わった操作の取り消しは driver に頼まない
    assert!(driver.cancelled.is_empty());
    let taken = take_op(&registry, &mut ctx, token, 0).unwrap();
    assert!(taken.cancelling && taken.result.is_err());
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// `blocking` の呼び出しの仕事を出す。答えは鍵を外さずに返る `u64` だけである。
#[cfg(test)]
fn submit_ffi_job(
    registry: &Shared,
    ctx: &mut Ctx<eml_rt::Checked>,
    driver: &mut NoDriver,
    me: CellRef,
) -> i64 {
    let job = Op::Job(Box::new(|| Box::new(7u64) as Box<dyn std::any::Any + Send>));
    submit_job(
        registry,
        &solo(registry),
        ctx,
        Some(&mut *driver),
        0,
        0,
        me,
        JobSource::C(3),
        job,
    )
    .unwrap()
}

/// プールの仕事の記録は完了の箱をそのまま持ち、取り出しが仕事の出どころとともに返して、`u64`に戻す
/// (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_job_record_takes_the_boxed_answer() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let token = submit_ffi_job(&registry, &mut ctx, &mut driver, me);
    assert_eq!(driver.submits, 1);
    assert_eq!(registry.lock().in_flight(0), 1);
    let mut out = Vec::new();
    complete_op(
        &registry,
        &mut ctx,
        &mut driver,
        CellRef::decode(token),
        Ok(Done::Job(Box::new(42u64))),
        &mut out,
    );
    assert!(out.is_empty());
    assert_eq!(registry.lock().in_flight(0), 0);
    let taken = take_job(&registry, &mut ctx, token, 0).unwrap();
    assert_eq!(taken.from, JobSource::C(3));
    assert_eq!(taken.answer::<u64>().unwrap().unwrap(), 42);
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 取り消しの地点で投げるタスクの受け付けは、仕事を driver に出さず、取り消しで終わった記録にする。待ちの数も予算も
/// 変えない。後の `io_wait` が数えるためである (docs/spec/vm.md の「`blocking` の呼び出し」)。
#[test]
fn a_cancelled_task_submits_no_job() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    registry.lock().task_mut(me).unwrap().cancel = true;
    let before = format!("{:?}", registry.lock().stats);
    let token = submit_ffi_job(&registry, &mut ctx, &mut driver, me);
    assert_eq!(driver.submits, 0);
    {
        let mut cells = registry.lock();
        assert_eq!(format!("{:?}", cells.stats), before);
        assert_eq!(cells.task_mut(me).unwrap().budget, BUDGET);
        assert_eq!(cells.in_flight(0), 0);
        let record = cells.op_mut(CellRef::decode(token)).unwrap();
        assert!(record.cancelling);
        assert!(matches!(record.phase, OpPhase::Done(Err(_), true)));
        assert!(record.pending_op.is_none());
    }
    let taken = take_job(&registry, &mut ctx, token, 0).unwrap();
    assert!(taken.result.is_err());
}

/// 完了は箱の型を見ない。`u64`でない値で終わった仕事を`u64`として取り出すのは、内部の誤りである。
#[test]
fn a_job_answer_of_another_type_is_an_internal_error() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let token = submit_ffi_job(&registry, &mut ctx, &mut driver, me);
    driver.done.push(Completion {
        key: op_key(CellRef::decode(token)),
        result: Ok(Done::Job(Box::new(42i32))),
        cancelled: false,
    });
    complete(
        &registry,
        &solo(&registry),
        &mut ctx,
        &mut driver,
        0,
        &mut Vec::new(),
        &mut Vec::new(),
    )
    .unwrap();
    let taken = take_job(&registry, &mut ctx, token, 0).unwrap();
    assert!(matches!(
        taken.answer::<u64>(),
        Err(CellFault::Internal(
            "an op completes with another kind of result"
        ))
    ));
}

/// `op_ready` はどの待ちでも予算を使い、1バイトの書きは1つ使う。受け付けで終わった記録では、取り消しの地点が
/// 投げなければ、予算が残れば待ちを1つ数えて続け、切れれば数えずに先に譲る。譲る待ちは続く `reschedule` が数える。終わっていない記録と、取り消しの
/// 地点が投げるときは待たせ、待ちも取り消しも数えない。待つ側の `park_io` が数えるためである
/// (docs/spec/vm.md の「操作の記録」)。
#[test]
fn op_ready_counts_a_wait_only_on_a_finished_op() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let pending = submit_x(&registry, &mut ctx, &mut driver, 0);
    assert_eq!(registry.lock().op_ready(pending, 0, me).unwrap(), FAST_WAIT);
    assert_eq!(
        {
            let mut cells = registry.lock();
            (cells.stats.parks, cells.task_mut(me).unwrap().budget)
        },
        (0, BUDGET - 1)
    );
    driver.finish_writes = true;
    let done = submit_x(&registry, &mut ctx, &mut driver, 0);
    assert_eq!(registry.lock().parts[0].in_flight, 1);
    assert_eq!(registry.lock().op_ready(done, 50, me).unwrap(), FAST_GO);
    let stats = registry.lock().stats;
    assert_eq!((stats.parks, stats.timers), (1, 1));
    registry.lock().task_mut(me).unwrap().budget = 1;
    assert_eq!(registry.lock().op_ready(done, 50, me).unwrap(), FAST_YIELD);
    let stats = registry.lock().stats;
    assert_eq!((stats.parks, stats.timers), (1, 2));
    assert!(registry.lock().task_mut(me).unwrap().fast_yield);
    registry.lock().task_mut(me).unwrap().fast_yield = false;
    registry.lock().task_mut(me).unwrap().cancel = true;
    assert_eq!(registry.lock().op_ready(done, 50, me).unwrap(), FAST_WAIT);
    assert_eq!(
        {
            let stats = registry.lock().stats;
            (stats.parks, stats.timers, stats.cancels)
        },
        (1, 2, 0)
    );
    let taken = take_op(&registry, &mut ctx, done, 0).unwrap();
    assert!(matches!(taken.result, Ok(1)));
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    registry.lock().parts[0].in_flight = 0;
    registry
        .lock()
        .op_mut(CellRef::decode(pending))
        .unwrap()
        .phase = OpPhase::Done(Ok(1), false);
    let taken = take_op(&registry, &mut ctx, pending, 0).unwrap();
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 書きの待ちは、1 に長さを 512 で割った商を足しただけ予算を使い、残りより多ければ残りをすべて使う。64 KiB の書きは
/// 1回で予算を使い切る。読みとほかの待ちは 1 だけ使う (docs/spec/stdlib.md の「操作の予算」)。
#[test]
fn writes_spend_budget_by_their_length() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver {
        finish_writes: true,
        ..NoDriver::default()
    };
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let mut spent = Vec::new();
    for len in [0, 511, 512, 1500] {
        let token = submit_bytes(&registry, &mut ctx, &mut driver, 0, &vec![1; len]);
        assert_eq!(registry.lock().op_ready(token, 0, me).unwrap(), FAST_GO);
        spent.push(BUDGET - registry.lock().task_mut(me).unwrap().budget);
        registry.lock().task_mut(me).unwrap().budget = BUDGET;
        let taken = take_op(&registry, &mut ctx, token, 0).unwrap();
        unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    }
    assert_eq!(spent, [1, 1, 2, 3]);
    let token = submit_bytes(&registry, &mut ctx, &mut driver, 0, &[1; 65536]);
    assert_eq!(registry.lock().op_ready(token, 0, me).unwrap(), FAST_YIELD);
    assert_eq!(registry.lock().task_mut(me).unwrap().budget, 0);
    let taken = take_op(&registry, &mut ctx, token, 0).unwrap();
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 予算を使い切った終わった操作の待ちが譲らずに続けると、譲った待ちの再開と同じく予算を戻し、`reschedule` の代わりに
/// 待ちを1つ数える。譲る印のない続けは内部の誤りである (docs/spec/stdlib.md の「操作の予算」)。
#[test]
fn going_on_after_a_spent_budget_counts_the_wait_and_refills() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver {
        finish_writes: true,
        ..NoDriver::default()
    };
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let done = submit_x(&registry, &mut ctx, &mut driver, 0);
    registry.lock().task_mut(me).unwrap().budget = 1;
    assert_eq!(registry.lock().op_ready(done, 0, me).unwrap(), FAST_YIELD);
    assert_eq!(registry.lock().stats.parks, 0);
    assert_eq!(registry.lock().go_on(me).unwrap(), FAST_GO);
    let mut cells = registry.lock();
    let task = cells.task_mut(me).unwrap();
    assert!(!task.fast_yield);
    assert_eq!(task.budget, BUDGET);
    assert_eq!(cells.stats.parks, 1);
    assert!(matches!(cells.go_on(me), Err(CellFault::Internal(_))));
    drop(cells);
    let taken = take_op(&registry, &mut ctx, done, 0).unwrap();
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 速い道が先に譲る `reschedule` の間に届いた取り消しは、その再開では数えず、次の取り消しの地点で1回だけ数える。
/// 操作の待ちからの再開は、予算を使い切った待ちのときだけ予算を戻す (docs/spec/stdlib.md の「操作の予算」)。
#[test]
fn a_cancel_during_a_fast_yield_is_counted_once() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver {
        finish_writes: true,
        ..NoDriver::default()
    };
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let done = submit_x(&registry, &mut ctx, &mut driver, 0);
    registry.lock().task_mut(me).unwrap().budget = 1;
    assert_eq!(registry.lock().op_ready(done, 0, me).unwrap(), FAST_YIELD);
    registry.lock().ready(me, Word::UNIT).unwrap();
    registry.lock().task_mut(me).unwrap().cancel = true;
    assert_eq!(registry.lock().resumed(me, WOKE, false).unwrap().0, WOKE);
    assert_eq!(registry.lock().stats.cancels, 0);
    assert_eq!(registry.lock().task_mut(me).unwrap().budget, BUDGET);
    // 次の取り消しの地点が1回だけ数える
    assert!(registry.lock().cancel_point(Some(me)).unwrap());
    assert_eq!(registry.lock().stats.cancels, 1);
    registry.lock().task_mut(me).unwrap().cancel = false;
    // 予算の残る操作の待ちからの再開は予算を戻さない
    registry.lock().task_mut(me).unwrap().budget = 5;
    registry.lock().task_mut(me).unwrap().phase = Phase::Parked;
    assert_eq!(registry.lock().resumed(me, WOKE, true).unwrap().0, WOKE);
    assert_eq!(registry.lock().task_mut(me).unwrap().budget, 5);
    registry.lock().task_mut(me).unwrap().budget = 0;
    registry.lock().task_mut(me).unwrap().phase = Phase::Parked;
    registry.lock().resumed(me, WOKE, true).unwrap();
    assert_eq!(registry.lock().task_mut(me).unwrap().budget, BUDGET);
    let taken = take_op(&registry, &mut ctx, done, 0).unwrap();
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 記録に触れるのは受け付けたタスクだけである。ほかの worker のタスクが token を渡すと内部の誤りになる
/// (docs/spec/vm.md の「操作の記録」)。
#[test]
fn an_op_used_by_another_task_is_an_internal_error() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver {
        finish_writes: true,
        ..NoDriver::default()
    };
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let other = running_task(&registry, scope);
    registry.lock().task_mut(other).unwrap().worker = 2;
    let token = submit_x(&registry, &mut ctx, &mut driver, 1);
    assert!(matches!(
        registry.lock().op_ready(token, 0, other),
        Err(CellFault::Internal("an op is used by another task"))
    ));
    assert!(matches!(
        registry.lock().op_of(token, 2),
        Err(CellFault::Internal("an op is used by another task"))
    ));
    let taken = take_op(&registry, &mut ctx, token, 1).unwrap();
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 取り出した記録は放すので、同じ token の2度目の取り出しは記録を指さない。token は即値で、型検査が1回だけ使うことを
/// 守るので、std の包みからは届かない (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_second_take_of_an_op_is_an_internal_error() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver {
        finish_writes: true,
        ..NoDriver::default()
    };
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let token = submit_x(&registry, &mut ctx, &mut driver, 0);
    let taken = take_op(&registry, &mut ctx, token, 0).unwrap();
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    assert!(matches!(
        take_op(&registry, &mut ctx, token, 0),
        Err(CellFault::Internal("an op names no record"))
    ));
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// run の終わりの `sweep_all` は、待つタスクが残した記録の buffer とストリームをタスクの ID の下で落としてから、
/// タスクのセルが ID を退かせる。記録を後に回すと、退かせるときに記録の物体が残って `TransferLeak` になる
/// (docs/spec/vm.md の「終わり」)。
#[test]
fn sweep_all_frees_a_waited_op_before_its_task() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let worker = unsafe { ctx.heap_mut() }.fork_worker();
    registry.lock().task_mut(me).unwrap().worker = worker;
    with_worker(&mut ctx, worker, 0, |ctx| {
        let token = submit_x(&registry, ctx, &mut driver, worker);
        let run = park_on(&registry, ctx, &mut driver, token, me).unwrap();
        assert!(run.is_none());
    });
    sweep_all(&registry, &mut ctx, 0).unwrap();
    assert_eq!(registry.lock().parts[0].in_flight, 0);
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 完了の鍵は記録の番号で、generation まで合わなければ記録に当てない。放した記録の添字を新しい記録が使い回しても、
/// 古い鍵の完了はその記録に届かず、内部の誤りになる (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_completion_with_a_stale_generation_names_no_op() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let old = submit_x(&registry, &mut ctx, &mut driver, 0);
    let stale = driver.last_key;
    driver.done.push(Completion {
        key: stale,
        result: Ok(Done::Size(1)),
        cancelled: false,
    });
    let mut out = Vec::new();
    complete(
        &registry,
        &solo(&registry),
        &mut ctx,
        &mut driver,
        0,
        &mut Vec::new(),
        &mut out,
    )
    .unwrap();
    let taken = take_op(&registry, &mut ctx, old, 0).unwrap();
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    let new = submit_x(&registry, &mut ctx, &mut driver, 0);
    let fresh = CellRef::decode(new);
    assert_eq!(fresh.index, CellRef::decode(stale as i64).index);
    assert_ne!(driver.last_key, stale);
    driver.done.push(Completion {
        key: stale,
        result: Ok(Done::Size(1)),
        cancelled: false,
    });
    let fault = complete(
        &registry,
        &solo(&registry),
        &mut ctx,
        &mut driver,
        0,
        &mut Vec::new(),
        &mut out,
    )
    .unwrap_err();
    assert!(
        matches!(fault, CellFault::Internal("a completion names no op")),
        "{fault:?}"
    );
}

/// 取り消しと受け付けは登録表の lock の区間で順に並ぶので、どちらかが必ず相手の印を見る。取り消す区間が host の
/// 取り出す区間より先なら、host が取り出すときに取り消しの印を読み、driver に出した直後に取り消しを頼む。後なら、
/// 取り消す側が driver に出した記録を見て、自分が host なら自分の driver に、そうでなければ host に頼む
/// (docs/spec/vm.md の「操作の記録」)。スレッドは使わず、2つの区間の順を表で並べる。
#[test]
fn a_cancel_and_a_submit_are_ordered_by_the_lock() {
    // (取り消しが先か, host, 取り消す持ち主, 取り消しの道, host が出した直後に取り消すか)
    let cases = [
        (true, 1, 0, CancelPath::Flag, true),
        (true, 0, 0, CancelPath::Flag, true),
        (false, 1, 0, CancelPath::Remote(1), false),
        (false, 0, 0, CancelPath::Local, false),
    ];
    for (cancel_first, host, me, path, cancel_now) in cases {
        let mut registry = Registry::new(Vec::new(), 2);
        let mut driver = NoDriver::default();
        let op = registry.alloc(CellKind::Op(net_record(OpKind::Accept, 0, None)));
        let key = op_key(op);
        registry.op_mut(op).unwrap().host = host;
        registry.queue_op(op, Op::Accept { listener: 0 }).unwrap();
        // 持ち主の取り消しの区間。印は道を決める前に立てる
        let cancel = |registry: &mut Registry| {
            registry.op_mut(op).unwrap().cancelling = true;
            registry.cancel_io(op, me).unwrap()
        };
        let (got_path, (next, got_now)) = if cancel_first {
            let got_path = cancel(&mut registry);
            (got_path, registry.take_queued(op).unwrap())
        } else {
            let taken = registry.take_queued(op).unwrap();
            (cancel(&mut registry), taken)
        };
        assert_eq!(
            (got_path, got_now),
            (path, cancel_now),
            "cancel first {cancel_first}, host {host}"
        );
        assert_eq!(registry.in_flight(host as usize), 1);
        // host は lock の外で driver に出し、取り出した区間で読んだ印に従って取り消す。取り消す側は自分が host のときだけ
        // 自分の driver に頼む
        assert!(matches!(driver.submit(key, next), Submit::Pending));
        if got_now {
            driver.cancel(key);
        }
        if got_path == CancelPath::Local {
            driver.cancel(key);
        }
        let expected: &[OpKey] = if got_now || got_path == CancelPath::Local {
            &[key]
        } else {
            &[]
        };
        assert_eq!(driver.cancelled, expected);
        // 結果を置くと、driver に出した数を戻す
        registry
            .arrive(op, Err(io::Error::other("cancelled")), true, host)
            .unwrap();
        assert_eq!(registry.in_flight(host as usize), 0);
        assert!(registry.op_mut(op).unwrap().arrival.is_some());
    }
}

/// 取り消しと受け付けが出会う道を、持ち主の `park_io`、`park_io_until`、`expire`、`unpark` と、host の `take_queued`
/// と `run_io` で通す。host が出す前の取り消しでは、持ち主の道は印を立てるだけで driver に頼まず、host が取り出す区間で
/// 印を読んで、出した操作が `Pending` なら直後に取り消す。期限の印 `timed_out` も同じに読む。出した後の取り消しでは、
/// 自分が host なら自分の driver に頼み、ほかの worker が host なら道は `Remote` になる。その場で終わった操作は取り消さない
/// (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_cancel_and_a_submit_meet_on_the_owner_and_host_paths() {
    #[derive(Debug, Clone, Copy)]
    enum Before {
        Cancel,
        PastDeadline,
        Expired,
    }
    for before in [Before::Cancel, Before::PastDeadline, Before::Expired] {
        let shared = Shared::new(Registry::new(Vec::new(), 2));
        let mut ctx = test_ctx();
        let mut driver = NoDriver::default();
        let scope = shared.lock().open_root(None, None).unwrap();
        let me = running_task(&shared, scope);
        let op = shared
            .lock()
            .alloc(CellKind::Op(net_record(OpKind::Accept, 0, None)));
        let token = op.encode();
        // 持ち主 0 が操作を置き、host 1 はまだ取り出していない
        {
            let mut cells = shared.lock();
            cells.op_mut(op).unwrap().host = 1;
            cells.queue_op(op, Op::Accept { listener: 0 }).unwrap();
        }
        let mut work = IoWork::default();
        match before {
            Before::Cancel => {
                let mut cells = shared.lock();
                cells.task_mut(me).unwrap().cancel = true;
                cells
                    .park_io(&solo(&shared), &mut work, token, me, Word::UNIT)
                    .unwrap();
            }
            Before::PastDeadline => {
                let mut cells = shared.lock();
                cells
                    .park_io_until(&solo(&shared), &mut work, 0, 0, token, me, Word::UNIT)
                    .unwrap();
            }
            Before::Expired => {
                let mut cells = shared.lock();
                cells
                    .park_io_until(&solo(&shared), &mut work, 0, 50, token, me, Word::UNIT)
                    .unwrap();
                cells
                    .expire(50, 0, &solo(&shared), &mut Vec::new(), &mut work)
                    .unwrap();
                assert!(cells.op_mut(op).unwrap().timed_out);
            }
        }
        assert!(work.cancels.is_empty(), "{before:?}");
        let (next, cancel_now) = shared.lock().take_queued(op).unwrap();
        assert!(cancel_now, "{before:?}");
        work.submits.push((op, next, cancel_now));
        let host = Here {
            me: 1,
            host: 1,
            mail: &shared.mail,
            post: &Unposted,
        };
        run_io(
            &shared,
            &host,
            &mut ctx,
            Some(&mut driver),
            0,
            &mut work,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(driver.cancelled, [op_key(op)], "{before:?}");
        assert_eq!(shared.lock().in_flight(1), 1);
    }

    // 出した後の取り消し。受け付けは表に入れる道で出す
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let scope = shared.lock().open_root(None, None).unwrap();
    let me = running_task(&shared, scope);
    let token = submit_x(&shared, &mut ctx, &mut driver, 0);
    let op = CellRef::decode(token);
    assert!(shared.lock().op_mut(op).unwrap().submitted);
    assert!(
        park_on(&shared, &mut ctx, &mut driver, token, me)
            .unwrap()
            .is_none()
    );
    assert!(driver.cancelled.is_empty());
    shared.lock().task_mut(me).unwrap().cancel = true;
    unpark(&shared, &mut driver, me, &mut Vec::new());
    assert_eq!(driver.cancelled, [op_key(op)]);
    shared.lock().op_mut(op).unwrap().host = 1;
    assert_eq!(
        shared.lock().cancel_io(op, 0).unwrap(),
        CancelPath::Remote(1)
    );

    // 取り出す前に期限が来ていても、その場で終わった操作は取り消さない
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let mut driver = NoDriver {
        finish_writes: true,
        ..NoDriver::default()
    };
    let op = shared.lock().alloc(CellKind::Op(net_record(
        OpKind::Write {
            len: 0,
            prefix: Prefix::Net,
        },
        0,
        None,
    )));
    shared.lock().op_mut(op).unwrap().timed_out = true;
    let mut work = IoWork::default();
    let empty = RawBuf {
        ptr: std::ptr::NonNull::dangling().as_ptr(),
        cap: 0,
    };
    shared
        .lock()
        .submit_op(
            op,
            Op::Write {
                handle: 0,
                buf: empty,
            },
            &solo(&shared),
            &mut work,
        )
        .unwrap();
    run_io(
        &shared,
        &solo(&shared),
        &mut ctx,
        Some(&mut driver),
        0,
        &mut work,
        &mut Vec::new(),
    )
    .unwrap();
    assert!(driver.cancelled.is_empty());
    assert!(matches!(
        shared.lock().op_mut(op).unwrap().phase,
        OpPhase::Done(Ok(0), false)
    ));
    assert_eq!(shared.lock().in_flight(0), 0);
}

/// `bytes` の読みを受け付けて待ち、端が読めるようになった完了を当てる。返すのは token と記録である。
#[cfg(test)]
fn probed_read(
    registry: &Shared,
    ctx: &mut Ctx<eml_rt::Checked>,
    driver: &mut NoDriver,
    me: CellRef,
    buf: Word,
) -> (i64, CellRef) {
    let stream = net_stream(ctx);
    let token = submit_read(
        registry,
        &solo(registry),
        ctx,
        Some(&mut *driver),
        0,
        0,
        buf,
        stream,
    )
    .unwrap();
    let op = CellRef::decode(token);
    assert!(
        park_on(registry, ctx, &mut *driver, token, me)
            .unwrap()
            .is_none()
    );
    let key = op_key(op);
    driver.done.push(Completion {
        key,
        result: Ok(Done::Size(0)),
        cancelled: false,
    });
    let mut out = Vec::new();
    complete(
        registry,
        &solo(registry),
        ctx,
        driver,
        0,
        &mut Vec::new(),
        &mut out,
    )
    .unwrap();
    // 読めるようになっただけでは再開せず、貸した記憶への読みを同じ鍵で待つ
    assert!(out.is_empty());
    assert!(matches!(
        registry.lock().op_mut(op).unwrap().phase,
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

/// 一意で空きが `MIN_ROOM` 以上ある buffer の読みは、待たずに同じ物体の空きを `READ_ROOM` バイトまで貸す。空きの
/// 少ない buffer の読みは、読めるようになってから中身を写した新しい物体を貸し、元を手放す。広げた物体に貸すのは
/// ちょうど `READ_ROOM` バイトである (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_read_lends_a_roomy_buffer_in_place_and_grows_a_short_one() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    for room in [2 * READ_ROOM, 16, 0] {
        let me = running_task(&registry, scope);
        let buf = bytes_ab(&mut ctx, room);
        let spare = unsafe { ctx.heap_mut().bytes_lend(buf) }.unwrap().cap;
        let in_place = spare >= MIN_ROOM;
        let submits = driver.submits;
        let (token, op) = if in_place {
            let stream = net_stream(&mut ctx);
            let token = submit_read(
                &registry,
                &solo(&registry),
                &mut ctx,
                Some(&mut driver),
                0,
                0,
                buf,
                stream,
            )
            .unwrap();
            let op = CellRef::decode(token);
            // 読めるようになるのを待つ操作を出さず、読みだけを出す
            assert_eq!(driver.submits, submits + 1);
            assert!(matches!(
                registry.lock().op_mut(op).unwrap().phase,
                OpPhase::Transfer
            ));
            (token, op)
        } else {
            let read = probed_read(&registry, &mut ctx, &mut driver, me, buf);
            assert_eq!(driver.submits, submits + 2);
            read
        };
        let lent = registry.lock().op_mut(op).unwrap().buf.unwrap();
        assert_eq!(lent == buf, in_place);
        let expected = if in_place {
            spare.min(READ_ROOM)
        } else {
            READ_ROOM
        };
        assert_eq!(driver.lent.pop().unwrap().cap, expected);
        let key = op_key(op);
        driver.done.push(Completion {
            key,
            result: Ok(Done::Size(0)),
            cancelled: false,
        });
        let mut out = Vec::new();
        complete(
            &registry,
            &solo(&registry),
            &mut ctx,
            &mut driver,
            0,
            &mut Vec::new(),
            &mut out,
        )
        .unwrap();
        let taken = take_op(&registry, &mut ctx, token, 0).unwrap();
        let back = taken.buf.unwrap();
        assert_eq!(back, lent);
        assert_eq!(unsafe { &*ctx.heap().bytes(back).unwrap() }, b"ab");
        unsafe { ctx.heap_mut().decref(back) }.unwrap();
    }
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 空きのない buffer を広げた容量は、長さが `READ_ROOM` より大きければ長さの2倍以上である。貸すのは容量によらず
/// `READ_ROOM` バイトである。足し続ける読みの写しを線形に収めるためである (docs/spec/vm.md の「操作の記録」)。
#[test]
fn growing_a_full_buffer_doubles_its_length() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let buf = unsafe { ctx.heap_mut() }.alloc_bytes(&vec![7; 200_000]);
    assert_eq!(unsafe { ctx.heap_mut().bytes_lend(buf) }.unwrap().cap, 0);
    let (token, op) = probed_read(&registry, &mut ctx, &mut driver, me, buf);
    assert_eq!(driver.lent.pop().unwrap().cap, READ_ROOM);
    let grown = registry.lock().op_mut(op).unwrap().buf.unwrap();
    // 長さ 200 000 の後ろの空きが 200 000 以上なので、容量は 400 000 以上である
    let spare = unsafe { ctx.heap_mut().bytes_lend(grown) }.unwrap().cap;
    assert!(spare >= 200_000, "{spare}");
    complete_op(
        &registry,
        &mut ctx,
        &mut driver,
        op,
        Ok(Done::Size(0)),
        &mut Vec::new(),
    );
    let back = take_op(&registry, &mut ctx, token, 0).unwrap().buf.unwrap();
    assert_eq!(unsafe { &*ctx.heap().bytes(back).unwrap() }.len(), 200_000);
    unsafe { ctx.heap_mut().decref(back) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 貸した記憶へ読んでいる間に取り消された読みも、外が書いたバイトを確定し、それを持つ buffer を返す
/// (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_read_cancelled_during_its_transfer_commits_the_bytes_it_got() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let buf = bytes_ab(&mut ctx, 0);
    let (token, op) = probed_read(&registry, &mut ctx, &mut driver, me, buf);
    let raw = driver.lent.pop().unwrap();
    // 外の書き手として、貸した空きの先頭に書く
    unsafe { std::ptr::copy_nonoverlapping(b"xy".as_ptr(), raw.ptr, 2) };
    registry.lock().task_mut(me).unwrap().cancel = true;
    let mut out = Vec::new();
    unpark(&registry, &mut driver, me, &mut out);
    let key = op_key(op);
    assert_eq!(driver.cancelled, [key]);
    // 取り消しが間に合わず、読みは2バイトで終わった
    driver.done.push(Completion {
        key,
        result: Ok(Done::Size(2)),
        cancelled: false,
    });
    complete(
        &registry,
        &solo(&registry),
        &mut ctx,
        &mut driver,
        0,
        &mut Vec::new(),
        &mut out,
    )
    .unwrap();
    assert!(matches!(
        out.as_slice(),
        [Runnable::ResumeIo { cell, wake: CANCELLED, .. }] if *cell == me
    ));
    let taken = take_op(&registry, &mut ctx, token, 0).unwrap();
    assert!(taken.cancelling && matches!(taken.result, Ok(2)));
    let back = taken.buf.unwrap();
    assert_eq!(unsafe { &*ctx.heap().bytes(back).unwrap() }, b"abxy");
    unsafe { ctx.heap_mut().decref(back) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 参照が2つある `Bytes` の書きは、写さずに同じ物体の中身を driver に貸し、`take_bytes` が同じ参照を返す。driver が
/// 読んだ中身は、もう1つの参照から読む中身と同じである (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_write_of_a_shared_buffer_makes_no_copy() {
    let mut ctx = write_of_a_shared_buffer::<eml_rt::Checked>();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
    write_of_a_shared_buffer::<eml_rt::Fast>();
}

#[cfg(test)]
fn write_of_a_shared_buffer<M: Mode>() -> Ctx<M> {
    let mut ctx = mode_ctx::<M>();
    let mut driver = NoDriver {
        finish_writes: true,
        ..NoDriver::default()
    };
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let buf = unsafe { ctx.heap_mut() }.alloc_bytes(b"shared");
    unsafe { ctx.heap_mut().dup(buf) }.unwrap();
    let stream = net_stream(&mut ctx);
    let token = submit_write(
        &registry,
        &solo(&registry),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        buf,
        stream,
    )
    .unwrap();
    let at = unsafe { ctx.heap().bytes(buf) }.unwrap();
    assert_eq!(driver.writes.len(), 1);
    assert_eq!(driver.writes[0].ptr, at.cast::<u8>().cast_mut());
    assert_eq!(driver.written, [b"shared".to_vec()]);
    let back = take_bytes(&registry, &mut ctx, token, 0).unwrap();
    assert!(back == buf);
    assert_eq!(unsafe { &*ctx.heap().bytes(back).unwrap() }, b"shared");
    unsafe { ctx.heap_mut().decref(back) }.unwrap();
    unsafe { ctx.heap_mut().decref(buf) }.unwrap();
    ctx
}

/// driver が保留にしている書きの buffer を、もう1つの参照で書き換えても、rc が 2 以上なので `Bytes.set`、`push`、
/// `append`、`truncate` は写してから書く。同じ物体への読みも空きを貸さず、読めるようになるのを待つ操作を出す。driver が
/// 完了を返すときに貸された記憶から読む中身は元のままで、書き換えた側だけが新しい中身を持つ
/// (docs/spec/object-model.md の T10 の行)。
#[test]
fn a_write_held_by_the_driver_is_not_changed_by_its_alias() {
    let mut ctx = held_write_and_its_alias::<eml_rt::Checked>();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
    held_write_and_its_alias::<eml_rt::Fast>();
}

#[cfg(test)]
fn held_write_and_its_alias<M: Mode>() -> Ctx<M> {
    let mut ctx = mode_ctx::<M>();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    // 空きのある物体にして、一意ならその場で書き換えられる形にする
    let buf = {
        let heap = unsafe { ctx.heap_mut() };
        let w = heap.alloc_bytes_room(64);
        unsafe { heap.bytes_extend(w, b"abc") }.unwrap()
    };
    unsafe { ctx.heap_mut().dup(buf) }.unwrap();
    let stream = net_stream(&mut ctx);
    let token = submit_write(
        &registry,
        &solo(&registry),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        buf,
        stream,
    )
    .unwrap();
    assert_eq!(driver.held.len(), 1);
    let lent = driver.held[0].1;
    let alias = unsafe { ctx.bytes_set(0, i64::from(b'z'), buf) }.unwrap();
    assert!(alias != buf);
    // ほかのその場で書き換える操作も、rc が 2 以上なので写す。どれも呼ぶ側の参照を1つ消費するので、先に1つ足す
    let dup = |ctx: &mut Ctx<M>| unsafe { ctx.heap_mut().dup(buf) }.unwrap();
    dup(&mut ctx);
    let pushed = unsafe { ctx.bytes_push(i64::from(b'd'), buf) }.unwrap();
    dup(&mut ctx);
    let tail = unsafe { ctx.heap_mut() }.alloc_bytes(b"de");
    let appended = unsafe { ctx.bytes_append(buf, tail) }.unwrap();
    dup(&mut ctx);
    let truncated = unsafe { ctx.bytes_truncate(1, buf) }.unwrap();
    for (w, content) in [
        (pushed, &b"abcd"[..]),
        (appended, b"abcde"),
        (truncated, b"a"),
    ] {
        assert!(w != buf);
        assert_eq!(unsafe { &*ctx.heap().bytes(w).unwrap() }, content);
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
    }
    // 同じ物体への読みは、一意でないので空きを貸さず、読めるようになるのを待つ操作を出す
    dup(&mut ctx);
    let reads = driver.lent.len();
    let other = net_stream(&mut ctx);
    let read = submit_read(
        &registry,
        &solo(&registry),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        buf,
        other,
    )
    .unwrap();
    assert_eq!(driver.lent.len(), reads);
    assert!(matches!(
        registry.lock().op_mut(CellRef::decode(read)).unwrap().phase,
        OpPhase::Probe
    ));
    driver.done.push(Completion {
        key: op_key(CellRef::decode(read)),
        result: Err(io::Error::from(io::ErrorKind::ConnectionReset)),
        cancelled: false,
    });
    complete(
        &registry,
        &solo(&registry),
        &mut ctx,
        &mut driver,
        0,
        &mut Vec::new(),
        &mut Vec::new(),
    )
    .unwrap();
    let taken = take_op(&registry, &mut ctx, read, 0).unwrap();
    assert!(taken.buf == Some(buf));
    unsafe { ctx.heap_mut().decref(buf) }.unwrap();
    assert!(driver.written.is_empty());
    assert_eq!(lent_content(lent), b"abc");
    driver.done.push(Completion {
        key: op_key(CellRef::decode(token)),
        result: Ok(Done::Size(3)),
        cancelled: false,
    });
    complete(
        &registry,
        &solo(&registry),
        &mut ctx,
        &mut driver,
        0,
        &mut Vec::new(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(driver.written, [b"abc".to_vec()]);
    let back = take_bytes(&registry, &mut ctx, token, 0).unwrap();
    assert!(back == buf);
    assert_eq!(unsafe { &*ctx.heap().bytes(back).unwrap() }, b"abc");
    assert_eq!(unsafe { &*ctx.heap().bytes(alias).unwrap() }, b"zbc");
    unsafe { ctx.heap_mut().decref(back) }.unwrap();
    unsafe { ctx.heap_mut().decref(alias) }.unwrap();
    ctx
}

/// 操作の待ちの期限は満ちても再開せずに driver に取り消しを頼み、記録に期限の印を付ける。取り消した操作の完了が
/// `Woke` で再開し、誤りの結果は期限切れとして読める (docs/spec/vm.md の「時間の待ち」)。
#[test]
fn an_expired_op_wait_cancels_the_op_and_resumes_on_its_completion() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let token = submit_x(&registry, &mut ctx, &mut driver, 0);
    let key = driver.last_key;
    let run = park_until_on(&registry, &mut ctx, &mut driver, 3, 50, token, me).unwrap();
    assert!(run.is_none());
    assert_eq!(
        {
            let cells = registry.lock();
            (cells.next_deadline(0), cells.stats.timers)
        },
        (Some(53), 1)
    );
    let mut out = Vec::new();
    expire(&registry, &mut driver, 0, 53, &mut out).unwrap();
    assert!(out.is_empty());
    assert_eq!(driver.cancelled, [key]);
    assert_eq!(registry.lock().next_deadline(0), None);
    driver.done.push(Completion {
        key,
        result: Err(io::Error::other("cancelled")),
        cancelled: true,
    });
    complete(
        &registry,
        &solo(&registry),
        &mut ctx,
        &mut driver,
        0,
        &mut Vec::new(),
        &mut out,
    )
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
    assert_eq!(registry.lock().op_outcome(op, 0).unwrap(), OUTCOME_LATE);
    let taken = take_op(&registry, &mut ctx, op, 0).unwrap();
    assert!(taken.timed_out && !taken.cancelling);
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 期限の付いた操作の待ちは、取り消しで外れても完了で終わっても輪の項を外す。項を残すと、満期に `expire` が待たない
/// タスクを名指す。
#[test]
fn cancelling_or_finishing_a_timed_op_wait_removes_its_timer() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let first = running_task(&registry, scope);
    let second = running_task(&registry, scope);
    let t1 = submit_x(&registry, &mut ctx, &mut driver, 0);
    let k1 = driver.last_key;
    let t2 = submit_x(&registry, &mut ctx, &mut driver, 0);
    let k2 = driver.last_key;
    park_until_on(&registry, &mut ctx, &mut driver, 0, 50, t1, first).unwrap();
    park_until_on(&registry, &mut ctx, &mut driver, 0, 60, t2, second).unwrap();
    registry.lock().task_mut(first).unwrap().cancel = true;
    let mut out = Vec::new();
    unpark(&registry, &mut driver, first, &mut out);
    assert!(out.is_empty());
    assert_eq!(driver.cancelled, [k1]);
    assert_eq!(registry.lock().next_deadline(0), Some(60));
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
    complete(
        &registry,
        &solo(&registry),
        &mut ctx,
        &mut driver,
        0,
        &mut Vec::new(),
        &mut out,
    )
    .unwrap();
    assert_eq!(registry.lock().next_deadline(0), None);
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
        let taken = take_op(&registry, &mut ctx, op, 0).unwrap();
        assert!(!taken.timed_out);
        unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    }
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 0 以下の期限は過ぎた期限なので、輪に載せずにすぐ driver に取り消しを頼み、記録に期限の印を付ける。
#[test]
fn a_past_deadline_cancels_the_op_without_a_timer() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let token = submit_x(&registry, &mut ctx, &mut driver, 0);
    let key = driver.last_key;
    let run = park_until_on(&registry, &mut ctx, &mut driver, 0, 0, token, me).unwrap();
    assert!(run.is_none());
    assert_eq!(driver.cancelled, [key]);
    let cells = registry.lock();
    assert_eq!((cells.next_deadline(0), cells.stats.timers), (None, 0));
    drop(cells);
    driver.done.push(Completion {
        key,
        result: Err(io::Error::other("cancelled")),
        cancelled: true,
    });
    let mut out = Vec::new();
    complete(
        &registry,
        &solo(&registry),
        &mut ctx,
        &mut driver,
        0,
        &mut Vec::new(),
        &mut out,
    )
    .unwrap();
    let Some(Runnable::ResumeIo { wake: WOKE, op, .. }) = out.pop() else {
        panic!("the op wait resumes with `Woke`");
    };
    assert_eq!(registry.lock().op_outcome(op, 0).unwrap(), OUTCOME_LATE);
    let taken = take_op(&registry, &mut ctx, op, 0).unwrap();
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 正の期限は、操作がもう終わっていて輪に載せないときも `timers` に数える。終わったかどうかはスケジュールで変わるので、
/// 数はプログラムだけで決まる (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn a_deadline_on_a_finished_op_counts_a_timer_without_one_on_the_wheel() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let token = submit_x(&registry, &mut ctx, &mut driver, 0);
    let key = driver.last_key;
    driver.done.push(Completion {
        key,
        result: Ok(Done::Size(1)),
        cancelled: false,
    });
    let mut out = Vec::new();
    complete(
        &registry,
        &solo(&registry),
        &mut ctx,
        &mut driver,
        0,
        &mut Vec::new(),
        &mut out,
    )
    .unwrap();
    assert!(out.is_empty());
    let run = park_until_on(&registry, &mut ctx, &mut driver, 0, 50, token, me).unwrap();
    assert!(matches!(
        run,
        Some(Runnable::ResumeIo { cell, wake: WOKE, .. }) if cell == me
    ));
    let cells = registry.lock();
    assert_eq!((cells.next_deadline(0), cells.stats.timers), (None, 1));
    drop(cells);
    assert!(driver.cancelled.is_empty());
    assert_eq!(registry.lock().op_outcome(token, 0).unwrap(), OUTCOME_OK);
    let taken = take_op(&registry, &mut ctx, token, 0).unwrap();
    assert!(matches!(taken.result, Ok(1)) && !taken.timed_out);
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 受けの記録を期限付きで待ち、期限を満たしてから `result` の完了を当てる。返すのは `op_outcome` の番号である。
/// どちらの番号も `finish` は投げず、包みが種類を決める。
#[cfg(test)]
fn expired_accept_outcome(result: io::Error, cancelled: bool) -> i64 {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let listener = NetListener {
        bind: Bind::Bound {
            host: 0,
            handle: 0,
            bin: Arc::default(),
        },
        port: 1,
    };
    let listener = unsafe { ctx.heap_mut() }.alloc_foreign(listener, true);
    let token = submit_accept(
        &registry,
        &solo(&registry),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        listener,
    )
    .unwrap();
    let key = driver.last_key;
    let run = park_until_on(&registry, &mut ctx, &mut driver, 0, 50, token, me).unwrap();
    assert!(run.is_none());
    let mut out = Vec::new();
    expire(&registry, &mut driver, 0, 50, &mut out).unwrap();
    assert_eq!(driver.cancelled, [key]);
    driver.done.push(Completion {
        key,
        result: Err(result),
        cancelled,
    });
    complete(
        &registry,
        &solo(&registry),
        &mut ctx,
        &mut driver,
        0,
        &mut Vec::new(),
        &mut out,
    )
    .unwrap();
    let Some(Runnable::ResumeIo { wake: WOKE, op, .. }) = out.pop() else {
        panic!("the op wait resumes with `Woke`");
    };
    let code = registry.lock().op_outcome(op, 0).unwrap();
    let back = registry.lock().op_stream(op, 0).unwrap();
    let taken = take_op(&registry, &mut ctx, op, 0).unwrap();
    assert!(taken.timed_out && taken.made.is_none() && taken.failure().is_none());
    unsafe { ctx.heap_mut().decref(back) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
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

/// 中身の buffer が空の `Std.Net.TcpStream`。
#[cfg(test)]
fn net_stream<M: Mode>(ctx: &mut Ctx<M>) -> Word {
    let stream = IoStream {
        bind: Bind::Bound {
            host: 0,
            handle: 0,
            bin: Arc::default(),
        },
        origin: Origin::Peer(([127, 0, 0, 1], 1).into()),
        rest: Vec::new(),
        start: 0,
    };
    unsafe { ctx.heap_mut() }.alloc_foreign(stream, true)
}

/// 受け付けを送った先だけを覚えるテストの宛先。
#[cfg(test)]
#[derive(Default)]
struct Submits(std::cell::RefCell<Vec<u32>>);

#[cfg(test)]
impl Post for Submits {
    fn wake(&self, k: u32, _w: Woken) {
        panic!("a wake is sent to worker {k}");
    }

    fn cancel(&self, k: u32, _cell: CellRef) {
        panic!("a cancel is sent to worker {k}");
    }

    fn place(&self, k: u32, _cell: CellRef, _worker: u32, _closure: Word) {
        panic!("a task is placed on worker {k}");
    }

    fn submit(&self, k: u32, _op: CellRef) {
        self.0.borrow_mut().push(k);
    }

    fn cancel_io(&self, k: u32, _op: CellRef) {
        panic!("an op cancel is sent to worker {k}");
    }

    fn completion(&self, k: u32, _op: CellRef) {
        panic!("a completion is sent to worker {k}");
    }

    fn limit(&self, k: u32, _cell: CellRef) {
        panic!("a limit is sent to worker {k}");
    }
}

/// 本物の時計の run では、worker 1 の接続は worker 1 の driver に出し、できたストリームを worker 1 に縛る。そのストリームを
/// 引く worker 1 の操作は `Submit` を送らずに自分の driver に出し、worker 0 のタスクの操作は host の worker 1 へ送る。
/// host は記録が持つストリームの縛りで決まり、受け付けた worker の `here.host` では決まらない
/// (docs/spec/vm.md の「時計と driver」)。
#[test]
fn a_stream_connected_on_worker_1_is_hosted_by_worker_1() {
    let mut ctx = test_ctx();
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    shared.lock().set_bin(1, Arc::default());
    let posted = Submits::default();
    let on = |me: u32| Here {
        me,
        host: me,
        mail: &shared.mail,
        post: &posted,
    };
    let host_of = |token: i64| shared.lock().op_mut(CellRef::decode(token)).unwrap().host;
    // worker 1 の接続は自分の driver に出す
    let mut driver_1 = NoDriver::default();
    let addr = unsafe { ctx.heap_mut() }.alloc_string("127.0.0.1");
    let token = submit_connect(
        &shared,
        &on(1),
        &mut ctx,
        Some(&mut driver_1),
        0,
        0,
        addr,
        80,
    )
    .unwrap()
    .unwrap();
    assert_eq!((host_of(token), driver_1.submits), (1, 1));
    driver_1.done.push(Completion {
        key: driver_1.last_key,
        result: Ok(Done::Stream {
            handle: 3,
            peer: ([127, 0, 0, 1], 80).into(),
        }),
        cancelled: false,
    });
    let mut out = Vec::new();
    complete(
        &shared,
        &on(1),
        &mut ctx,
        &mut driver_1,
        0,
        &mut Vec::new(),
        &mut out,
    )
    .unwrap();
    let stream = take_made(&shared, &mut ctx, token, 0, Made::Stream).unwrap();
    assert_eq!(
        unsafe { &*ctx.heap().foreign_unique::<IoStream>(stream).unwrap() }
            .bind
            .host(),
        Some(1)
    );
    // そのストリームを引く worker 1 の操作は、知らせを送らずに worker 1 の driver に出す
    let buf = unsafe { ctx.heap_mut() }.alloc_bytes_room(64);
    let read = submit_read(
        &shared,
        &on(1),
        &mut ctx,
        Some(&mut driver_1),
        0,
        0,
        buf,
        stream,
    )
    .unwrap();
    assert_eq!((host_of(read), driver_1.submits), (1, 2));
    assert!(posted.0.borrow().is_empty());
    // worker 1 に縛ったストリームを引く worker 0 の操作は、worker 0 の driver に出さずに worker 1 へ送る
    let mut driver_0 = NoDriver::default();
    let other = unsafe { ctx.heap_mut() }.alloc_foreign(
        IoStream {
            bind: Bind::Bound {
                host: 1,
                handle: 4,
                bin: Arc::default(),
            },
            origin: Origin::Peer(([127, 0, 0, 1], 1).into()),
            rest: Vec::new(),
            start: 0,
        },
        true,
    );
    let bytes = unsafe { ctx.heap_mut() }.alloc_bytes_room(8);
    let write = submit_write(
        &shared,
        &on(0),
        &mut ctx,
        Some(&mut driver_0),
        0,
        0,
        bytes,
        other,
    )
    .unwrap();
    assert_eq!((host_of(write), driver_0.submits), (1, 0));
    assert_eq!(*posted.0.borrow(), [1]);
}

/// `Std.Stream` が開いた、ファイル `f.txt` の縛ったストリーム。
#[cfg(test)]
fn file_stream<M: Mode>(ctx: &mut Ctx<M>) -> Word {
    let stream = IoStream {
        bind: Bind::Bound {
            host: 0,
            handle: 0,
            bin: Arc::default(),
        },
        origin: Origin::File("f.txt".into()),
        rest: Vec::new(),
        start: 0,
    };
    unsafe { ctx.heap_mut() }.alloc_foreign(stream, true)
}

/// ファイルの読みは、空きの少ない buffer でも読めるようになるのを待たない。受け付けで中身を写した新しい物体を作り、
/// その空きを `READ_ROOM` バイト貸して、読みを1つだけ出す (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_read_of_a_file_grows_at_submit() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let buf = bytes_ab(&mut ctx, 0);
    let stream = file_stream(&mut ctx);
    let token = submit_read(
        &registry,
        &solo(&registry),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        buf,
        stream,
    )
    .unwrap();
    let op = CellRef::decode(token);
    assert_eq!(driver.submits, 1);
    assert!(matches!(
        registry.lock().op_mut(op).unwrap().phase,
        OpPhase::Transfer
    ));
    let raw = driver.lent.pop().unwrap();
    assert_eq!(raw.cap, READ_ROOM);
    unsafe { std::ptr::copy_nonoverlapping(b"cd".as_ptr(), raw.ptr, 2) };
    let mut out = Vec::new();
    complete_op(
        &registry,
        &mut ctx,
        &mut driver,
        op,
        Ok(Done::Size(2)),
        &mut out,
    );
    let taken = take_op(&registry, &mut ctx, token, 0).unwrap();
    let back = taken.buf.unwrap();
    assert_eq!(unsafe { &*ctx.heap().bytes(back).unwrap() }, b"abcd");
    unsafe { ctx.heap_mut().decref(back) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// ファイルの行の連鎖は、受け付けでも、行のない周の後でも、空きがなければストリームの buffer を広げて貸す。読めるように
/// なるのを待つ操作は出さない (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_read_line_of_a_file_grows_at_submit() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let stream = file_stream(&mut ctx);
    let token = submit_read_line(
        &registry,
        &solo(&registry),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        stream,
    )
    .unwrap();
    let op = CellRef::decode(token);
    assert_eq!(driver.submits, 1);
    let raw = driver.lent.pop().unwrap();
    assert_eq!(raw.cap, READ_ROOM);
    // 貸した空きを改行なしで埋めると、次の周は空きのない buffer を広げて貸す
    let filler = vec![b'a'; READ_ROOM];
    unsafe { std::ptr::copy_nonoverlapping(filler.as_ptr(), raw.ptr, READ_ROOM) };
    let mut out = Vec::new();
    complete_op(
        &registry,
        &mut ctx,
        &mut driver,
        op,
        Ok(Done::Size(READ_ROOM)),
        &mut out,
    );
    assert_eq!(driver.submits, 2);
    assert!(matches!(
        registry.lock().op_mut(op).unwrap().phase,
        OpPhase::Transfer
    ));
    assert!(driver.lent.pop().unwrap().cap >= MIN_ROOM);
    complete_op(
        &registry,
        &mut ctx,
        &mut driver,
        op,
        Ok(Done::Size(0)),
        &mut out,
    );
    let line = take_line(&registry, &mut ctx, token, 0).unwrap().unwrap();
    assert_eq!(
        unsafe { (&*ctx.heap().string(line).unwrap()).len() },
        READ_ROOM
    );
    unsafe { ctx.heap_mut().decref(line) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 共有の buffer のファイルの読みは、受け付けで1回だけ写した新しい物体に読む。元の物体はほかの参照から見て変わらない
/// (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_read_of_a_shared_buffer_on_a_file_copies_once() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let buf = bytes_ab(&mut ctx, 2 * READ_ROOM);
    unsafe { ctx.heap_mut().dup(buf) }.unwrap();
    let stream = file_stream(&mut ctx);
    let token = submit_read(
        &registry,
        &solo(&registry),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        buf,
        stream,
    )
    .unwrap();
    let op = CellRef::decode(token);
    assert_eq!(driver.submits, 1);
    let lent = registry.lock().op_mut(op).unwrap().buf.unwrap();
    assert_ne!(lent, buf);
    let raw = driver.lent.pop().unwrap();
    assert_eq!(raw.cap, READ_ROOM);
    unsafe { std::ptr::copy_nonoverlapping(b"cd".as_ptr(), raw.ptr, 2) };
    let mut out = Vec::new();
    complete_op(
        &registry,
        &mut ctx,
        &mut driver,
        op,
        Ok(Done::Size(2)),
        &mut out,
    );
    let taken = take_op(&registry, &mut ctx, token, 0).unwrap();
    let back = taken.buf.unwrap();
    assert_eq!(back, lent);
    assert_eq!(unsafe { &*ctx.heap().bytes(back).unwrap() }, b"abcd");
    assert_eq!(unsafe { &*ctx.heap().bytes(buf).unwrap() }, b"ab");
    for w in [back, buf] {
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
    }
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 受け付けと完了の知らせを覚えるテストの宛先。
#[cfg(test)]
#[derive(Default)]
struct OpMail {
    submits: std::cell::RefCell<Vec<(u32, CellRef)>>,
    completions: std::cell::RefCell<Vec<(u32, CellRef)>>,
}

#[cfg(test)]
impl Post for OpMail {
    fn wake(&self, k: u32, _w: Woken) {
        panic!("a wake is sent to worker {k}");
    }

    fn cancel(&self, k: u32, _cell: CellRef) {
        panic!("a cancel is sent to worker {k}");
    }

    fn place(&self, k: u32, _cell: CellRef, _worker: u32, _closure: Word) {
        panic!("a task is placed on worker {k}");
    }

    fn submit(&self, k: u32, op: CellRef) {
        self.submits.borrow_mut().push((k, op));
    }

    fn cancel_io(&self, k: u32, _op: CellRef) {
        panic!("an op cancel is sent to worker {k}");
    }

    fn completion(&self, k: u32, op: CellRef) {
        self.completions.borrow_mut().push((k, op));
    }

    fn limit(&self, k: u32, _cell: CellRef) {
        panic!("a limit is sent to worker {k}");
    }
}

/// worker 1 のタスクが、このクレートの `Cargo.toml` を `Std.Stream.open` で開く記録を出す。
#[cfg(test)]
fn open_on_worker_1(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<eml_rt::Checked>,
    driver: Option<&mut (dyn IoDriver + 'static)>,
) -> i64 {
    let path =
        unsafe { ctx.heap_mut() }.alloc_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
    submit_open(shared, here, ctx, driver, 0, 0, path, 0)
        .unwrap()
        .unwrap()
}

/// worker 0 が host として、受け箱の `Submit` の記録 `op` を自分の driver に出す。
#[cfg(test)]
fn host_takes_submit(
    shared: &Shared,
    here: &Here,
    ctx: &mut Ctx<eml_rt::Checked>,
    driver: &mut NoDriver,
    op: CellRef,
) {
    let mut work = IoWork::default();
    shared.lock().take_submit(op, here.me, &mut work).unwrap();
    let mut out = Vec::new();
    run_io(shared, here, ctx, Some(driver), 0, &mut work, &mut out).unwrap();
    assert!(out.is_empty());
}

/// ファイルは開くときに、開いたタスクの I/O の host の driver に縛る。仮想の時計では worker 1 の記録を worker 0 が
/// 取り込み、ストリームは worker 0 に縛る。本物の時計では worker 1 が自分の driver に取り込み、受け付けで終える
/// (docs/spec/vm.md の「時計と driver」)。
#[test]
fn an_open_record_binds_the_file_to_the_openers_host() {
    let mut ctx = test_ctx();
    let host_of = |ctx: &Ctx<eml_rt::Checked>, stream: Word| {
        unsafe { &*ctx.heap().foreign_unique::<IoStream>(stream).unwrap() }
            .bind
            .host()
    };
    // 仮想の時計: worker 1 の I/O の host は worker 0 である
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let posted = OpMail::default();
    let on = |me: u32, host: u32| Here {
        me,
        host,
        mail: &shared.mail,
        post: &posted,
    };
    let token = open_on_worker_1(&shared, &on(1, 0), &mut ctx, None);
    let op = CellRef::decode(token);
    assert_eq!(*posted.submits.borrow(), [(0, op)]);
    let mut driver_0 = NoDriver::default();
    host_takes_submit(&shared, &on(0, 0), &mut ctx, &mut driver_0, op);
    assert_eq!((driver_0.submits, driver_0.adopted), (1, 1));
    // 待つタスクのいない記録には `Completion` を送らず、持ち主が次に読むときに当てる
    assert!(posted.completions.borrow().is_empty());
    let woken = shared
        .lock()
        .settle_arrived(&mut ctx, 0, op, &on(1, 0), &mut IoWork::default())
        .unwrap();
    assert!(woken.is_none());
    let reader = take_made(&shared, &mut ctx, token, 0, Made::Reader).unwrap();
    assert_eq!(host_of(&ctx, reader), Some(0));
    unsafe { ctx.heap_mut().decref(reader) }.unwrap();
    // 本物の時計: worker 1 は自分の driver に取り込む
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let posted = OpMail::default();
    let here = Here {
        me: 1,
        host: 1,
        mail: &shared.mail,
        post: &posted,
    };
    let mut driver_1 = NoDriver::default();
    let token = open_on_worker_1(&shared, &here, &mut ctx, Some(&mut driver_1));
    assert!(posted.submits.borrow().is_empty());
    assert_eq!((driver_1.submits, driver_1.adopted), (1, 1));
    let reader = take_made(&shared, &mut ctx, token, 0, Made::Reader).unwrap();
    assert_eq!(host_of(&ctx, reader), Some(1));
    unsafe { ctx.heap_mut().decref(reader) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// host の取り込みを待つ間に取り消された開く記録も、`connect` と同じく完了からストリームを作る。包みの `finish` が
/// それを落とすと、ハンドルは host の bin に積まれる (docs/spec/vm.md の「操作の記録」)。
#[test]
fn an_open_cancelled_after_adoption_bins_its_handle() {
    let mut ctx = test_ctx();
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let posted = OpMail::default();
    let on = |me: u32| Here {
        me,
        host: 0,
        mail: &shared.mail,
        post: &posted,
    };
    let me = task_on(&shared, 1, 0, false);
    let token = open_on_worker_1(&shared, &on(1), &mut ctx, None);
    let op = CellRef::decode(token);
    // 待ち始めたタスクに取り消しが届く。host はまだ受け付けていないので、印だけを付ける
    shared.lock().task_mut(me).unwrap().cancel = true;
    let run = park_io(&shared, &on(1), &mut ctx, None, 0, token, me, Word::UNIT).unwrap();
    assert!(run.is_none());
    assert!(shared.lock().op_mut(op).unwrap().cancelling);
    let mut driver_0 = NoDriver::default();
    host_takes_submit(&shared, &on(0), &mut ctx, &mut driver_0, op);
    assert_eq!(driver_0.adopted, 1);
    assert_eq!(*posted.completions.borrow(), [(1, op)]);
    let woken = shared
        .lock()
        .take_completion(&mut ctx, 0, op, &on(1), &mut IoWork::default())
        .unwrap();
    assert!(woken.is_some());
    // `finish` と同じく記録を放し、作ったストリームを落とす
    let taken = take_op(&shared, &mut ctx, token, 0).unwrap();
    assert!(taken.cancelling);
    assert_eq!(taken.outcome(), OUTCOME_OK);
    let made = taken.made.unwrap();
    unsafe { ctx.heap_mut().decref(made) }.unwrap();
    let bin = shared.lock().host_bin(0, Arc::default()).unwrap();
    assert_eq!(*bin.bin.dropped.lock().unwrap(), [100]);
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// ループバックで受けた、どの driver にも取り込んでいない OS のソケットと、その相手。
#[cfg(test)]
fn accepted_socket() -> (eml_rt::RawSocket, std::net::TcpStream) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let near = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (far, _) = listener.accept().unwrap();
    (eml_rt::RawSocket::new(eml_rt::OsSocket::from(far)), near)
}

/// worker 0 が受けた記録の完了 `Done::Accepted` を当てて、縛っていないストリームを取り出す。
#[cfg(test)]
fn accept_unbound(
    shared: &Shared,
    ctx: &mut Ctx<eml_rt::Checked>,
    sock: eml_rt::RawSocket,
) -> Word {
    let mut driver_0 = NoDriver::default();
    let listener = unsafe { ctx.heap_mut() }.alloc_foreign(
        NetListener {
            bind: Bind::Bound {
                host: 0,
                handle: 0,
                bin: Arc::default(),
            },
            port: 1,
        },
        true,
    );
    let token = submit_accept(
        shared,
        &solo(shared),
        ctx,
        Some(&mut driver_0),
        0,
        0,
        listener,
    )
    .unwrap();
    driver_0.done.push(Completion {
        key: driver_0.last_key,
        result: Ok(Done::Accepted {
            sock,
            peer: ([127, 0, 0, 1], 80).into(),
        }),
        cancelled: false,
    });
    complete(
        shared,
        &solo(shared),
        ctx,
        &mut driver_0,
        0,
        &mut Vec::new(),
        &mut Vec::new(),
    )
    .unwrap();
    let stream = take_made(shared, ctx, token, 0, Made::Stream).unwrap();
    assert!(stream_unbound(ctx, stream).unwrap());
    stream
}

/// 本物の時計の run で、worker 0 のリスナーで受けたストリームは縛らずに持ち主へ渡り、最初に操作を受け付けた worker 1 の
/// driver に取り込まれる。その操作も後の操作も、知らせを送らずに worker 1 の driver に出す (docs/spec/vm.md の
/// 「操作の記録」)。
#[test]
fn an_accepted_stream_is_hosted_by_the_worker_that_uses_it_first() {
    let mut ctx = test_ctx();
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    shared.lock().set_bin(1, Arc::default());
    let posted = Submits::default();
    let on = |me: u32| Here {
        me,
        host: me,
        mail: &shared.mail,
        post: &posted,
    };
    let host_of = |token: i64| shared.lock().op_mut(CellRef::decode(token)).unwrap().host;
    let (sock, _peer) = accepted_socket();
    let stream = accept_unbound(&shared, &mut ctx, sock);
    let mut driver_1 = NoDriver::default();
    let buf = unsafe { ctx.heap_mut() }.alloc_bytes_room(64);
    let read = submit_read(
        &shared,
        &on(1),
        &mut ctx,
        Some(&mut driver_1),
        0,
        0,
        buf,
        stream,
    )
    .unwrap();
    assert_eq!(
        (host_of(read), driver_1.adopted, driver_1.submits),
        (1, 1, 1)
    );
    assert!(posted.0.borrow().is_empty());
    // 縛ったストリームの次の操作は、取り込み直さずに同じハンドルで worker 1 の driver に出す
    let s = unsafe { &*ctx.heap().foreign_unique::<IoStream>(stream).unwrap() };
    assert_eq!((s.bind.host(), s.bind.handle().unwrap()), (Some(1), 100));
}

/// 取り込みを driver が断ったストリームは、その操作もそれから後の操作も、driver に出さずに失敗で終わる
/// (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_stream_that_a_driver_refuses_fails_every_op() {
    let mut ctx = test_ctx();
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let (sock, _peer) = accepted_socket();
    let stream = accept_unbound(&shared, &mut ctx, sock);
    let mut driver = NoDriver {
        refuse_adopt: true,
        ..NoDriver::default()
    };
    let bytes = unsafe { ctx.heap_mut() }.alloc_bytes_room(8);
    let write = submit_write(
        &shared,
        &solo(&shared),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        bytes,
        stream,
    )
    .unwrap();
    assert_eq!(driver.submits, 0);
    assert_eq!(shared.lock().op_outcome(write, 0).unwrap(), OUTCOME_FAILED);
    let stream = shared.lock().op_stream(write, 0).unwrap();
    let taken = take_op(&shared, &mut ctx, write, 0).unwrap();
    let (_, message) = taken.failure().unwrap();
    assert_eq!(
        message,
        "cannot write to the stream: the driver refused the socket"
    );
    for w in [taken.buf, taken.made].into_iter().flatten() {
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
    }
    let line = submit_read_line(
        &shared,
        &solo(&shared),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        stream,
    )
    .unwrap();
    assert_eq!(driver.submits, 0);
    let stream = shared.lock().op_stream(line, 0).unwrap();
    let taken = take_op(&shared, &mut ctx, line, 0).unwrap();
    let (e, _) = taken.failure().unwrap();
    assert_eq!(e.kind(), io::ErrorKind::NotConnected);
    unsafe { ctx.heap_mut().decref(stream) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// ストリーム `stream` の buffer。
#[cfg(test)]
fn net_rest(ctx: &Ctx<eml_rt::Checked>, stream: Word) -> &IoStream {
    unsafe { &*ctx.heap().foreign_unique::<IoStream>(stream).unwrap() }
}

/// 記録 `op` の今の鍵の操作を `result` で終わらせ、集める地点で当てる。
#[cfg(test)]
fn complete_op(
    registry: &Shared,
    ctx: &mut Ctx<eml_rt::Checked>,
    driver: &mut NoDriver,
    op: CellRef,
    result: io::Result<Done>,
    out: &mut Vec<Runnable>,
) {
    let key = op_key(op);
    driver.done.push(Completion {
        key,
        result,
        cancelled: false,
    });
    complete(
        registry,
        &solo(registry),
        ctx,
        driver,
        0,
        &mut Vec::new(),
        out,
    )
    .unwrap();
}

/// 行の連鎖の読みを、外の書き手が `bytes` を書き終えるまで進める。周が読めるようになるのを待っていれば、その完了を
/// 当てる。貸されたストリームの buffer の空きに外の書き手として書けるだけ書き、その長さで読みを終わらせる。空きが
/// `bytes` より少なければ、次の周で残りを書く。返すのは、最初の周が読めるようになるのを待ったかと、最初の周に貸された
/// 空きのバイト数と、周ごとに書いたバイト数である。
#[cfg(test)]
fn read_line_round(
    registry: &Shared,
    ctx: &mut Ctx<eml_rt::Checked>,
    driver: &mut NoDriver,
    op: CellRef,
    bytes: &[u8],
    out: &mut Vec<Runnable>,
) -> (bool, usize, Vec<usize>) {
    let mut first = None;
    let mut reads = Vec::new();
    let mut rest = bytes;
    loop {
        let probed = matches!(registry.lock().op_mut(op).unwrap().phase, OpPhase::Probe);
        if probed {
            complete_op(registry, ctx, driver, op, Ok(Done::Size(0)), out);
        }
        assert!(matches!(
            registry.lock().op_mut(op).unwrap().phase,
            OpPhase::Transfer
        ));
        let raw = driver.lent.pop().unwrap();
        assert!(driver.lent.is_empty());
        first.get_or_insert((probed, raw.cap));
        let n = raw.cap.min(rest.len());
        unsafe { std::ptr::copy_nonoverlapping(rest.as_ptr(), raw.ptr, n) };
        complete_op(registry, ctx, driver, op, Ok(Done::Size(n)), out);
        reads.push(n);
        rest = &rest[n..];
        if rest.is_empty() {
            let (probed, room) = first.unwrap();
            return (probed, room, reads);
        }
        assert!(out.is_empty());
    }
}

/// 行の連鎖は記録の buffer でなくストリームの buffer の空きを貸し、外が書いたバイトから1行を切り出す。空のストリーム
/// は読めるようになってから buffer を広げて `READ_ROOM` バイトを貸す。読みすぎたバイトは buffer に残り、次の
/// `read_line` は返したバイトを詰めてから、残った空きを待たずに貸す。残りがなくなれば確保を放す
/// (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_read_line_lends_the_stream_buffer_and_frees_it_when_empty() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let mut stream = net_stream(&mut ctx);
    for (bytes, line, left, probed) in
        [(&b"ab\ncd"[..], "ab\n", 2, true), (b"\n", "cd\n", 0, false)]
    {
        let me = running_task(&registry, scope);
        let token = submit_read_line(
            &registry,
            &solo(&registry),
            &mut ctx,
            Some(&mut driver),
            0,
            0,
            stream,
        )
        .unwrap();
        let op = CellRef::decode(token);
        assert!(
            park_on(&registry, &mut ctx, &mut driver, token, me)
                .unwrap()
                .is_none()
        );
        let mut out = Vec::new();
        let (waited, room, _) =
            read_line_round(&registry, &mut ctx, &mut driver, op, bytes, &mut out);
        // 2周目は、1周目に広げた buffer の残った空きを貸す
        assert_eq!(waited, probed);
        assert!(if probed {
            room == READ_ROOM
        } else {
            room >= MIN_ROOM
        });
        assert!(matches!(
            out.as_slice(),
            [Runnable::ResumeIo { cell, wake: WOKE, .. }] if *cell == me
        ));
        assert!(registry.lock().op_mut(op).unwrap().buf.is_none());
        stream = registry.lock().op_stream(token, 0).unwrap();
        let taken = take_line(&registry, &mut ctx, token, 0).unwrap().unwrap();
        assert_eq!(unsafe { &*ctx.heap().string(taken).unwrap() }, line);
        unsafe { ctx.heap_mut().decref(taken) }.unwrap();
        let s = net_rest(&ctx, stream);
        assert_eq!(s.pending().len(), left);
        assert_eq!(s.rest.capacity() == 0, left == 0);
    }
    unsafe { ctx.heap_mut().decref(stream) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 読んでいる間に取り消された行の連鎖は、外が書いたバイトをストリームの buffer に確定してから、取り消しだけで終わった
/// 記録として `Cancelled` で再開する。読んだ途中の行は buffer に残る (docs/spec/stdlib.md の「`Std.Net`」)。
#[test]
fn a_read_line_cancelled_during_its_transfer_keeps_the_bytes() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let stream = net_stream(&mut ctx);
    let token = submit_read_line(
        &registry,
        &solo(&registry),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        stream,
    )
    .unwrap();
    let op = CellRef::decode(token);
    park_on(&registry, &mut ctx, &mut driver, token, me).unwrap();
    let mut out = Vec::new();
    complete_op(
        &registry,
        &mut ctx,
        &mut driver,
        op,
        Ok(Done::Size(0)),
        &mut out,
    );
    let raw = driver.lent.pop().unwrap();
    unsafe { std::ptr::copy_nonoverlapping(b"xy".as_ptr(), raw.ptr, 2) };
    registry.lock().task_mut(me).unwrap().cancel = true;
    unpark(&registry, &mut driver, me, &mut out);
    assert_eq!(driver.cancelled, [op_key(op)]);
    // 取り消しが間に合わず、読みは2バイトで終わった
    complete_op(
        &registry,
        &mut ctx,
        &mut driver,
        op,
        Ok(Done::Size(2)),
        &mut out,
    );
    assert!(matches!(
        out.as_slice(),
        [Runnable::ResumeIo { cell, wake: CANCELLED, .. }] if *cell == me
    ));
    let back = registry.lock().op_stream(token, 0).unwrap();
    let taken = take_op(&registry, &mut ctx, token, 0).unwrap();
    assert!(taken.cancelling && taken.result.is_err() && taken.failure().is_none());
    assert_eq!(net_rest(&ctx, back).pending(), b"xy");
    unsafe { ctx.heap_mut().decref(back) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 行の連鎖を、`chunks` を順に外の書き手として書いて進める。1つの塊は、貸された空きが足りなければ2つ以上の周に
/// 分かれる。最後の塊の前には終わらず、最後の塊で終わることを確かめる。返すのは、成功なら記録が写した行の文字列の
/// バイト数、失敗なら `finish` が投げる文言と、周ごとに読んだバイト数である。
#[cfg(test)]
fn read_line_through(chunks: &[&[u8]]) -> (Result<usize, String>, Vec<usize>) {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let stream = net_stream(&mut ctx);
    let token = submit_read_line(
        &registry,
        &solo(&registry),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        stream,
    )
    .unwrap();
    let op = CellRef::decode(token);
    park_on(&registry, &mut ctx, &mut driver, token, me).unwrap();
    let mut out = Vec::new();
    let mut reads = Vec::new();
    for chunk in chunks {
        assert!(out.is_empty());
        let (_, _, round) = read_line_round(&registry, &mut ctx, &mut driver, op, chunk, &mut out);
        reads.extend(round);
    }
    assert!(matches!(
        out.as_slice(),
        [Runnable::ResumeIo { cell, wake: WOKE, .. }] if *cell == me
    ));
    let back = registry.lock().op_stream(token, 0).unwrap();
    let ok = registry.lock().op_outcome(token, 0).unwrap() == OUTCOME_OK;
    let taken = take_op(&registry, &mut ctx, token, 0).unwrap();
    let result = match (&taken.kind, taken.made, taken.failure()) {
        (OpKind::Line { .. }, Some(line), None) if ok => {
            let n = unsafe { &*ctx.heap().string(line).unwrap() }.len();
            unsafe { ctx.heap_mut().decref(line) }.unwrap();
            Ok(n)
        }
        (_, None, Some((e, message))) if !ok => {
            assert_eq!(e.kind(), io::ErrorKind::Other);
            Err(message)
        }
        _ => panic!("the line op ends with a line or a failure"),
    };
    unsafe { ctx.heap_mut().decref(back) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
    (result, reads)
}

/// 上限の失敗の文言。
#[cfg(test)]
const TOO_LONG: &str = "cannot read the stream: a line is longer than 1048576 bytes";

/// 中身がちょうど `LINE_LIMIT` バイトの行は、終わりの `\r\n` を足しても受け付ける。改行の来る前の最後の `\r` は終わりの
/// 始まりかもしれないので、まだ中身に数えない (docs/spec/stdlib.md の「`Std.Net`」)。
#[test]
fn a_read_line_of_exactly_the_limit_is_accepted() {
    let chunk = vec![b'a'; READ_ROOM];
    let mut chunks = vec![&chunk[..]; LINE_LIMIT / READ_ROOM];
    chunks.extend([&b"\r"[..], b"\n"]);
    assert_eq!(read_line_through(&chunks).0, Ok(LINE_LIMIT + 2));
}

/// 上限を越える読みが改行を運んできても、行の中身が `LINE_LIMIT` バイトを超えれば失敗する。結果はバイトが読みにどう
/// 分かれて届いたかによらない (docs/spec/stdlib.md の「`Std.Net`」)。広げ方が変わってもこの場合を確かめ続けるように、
/// 最後の読みの前の中身が上限以内で、改行を運ぶ最後の読みが上限を越えることも確かめる。
#[test]
fn a_read_line_whose_newline_crosses_the_limit_fails() {
    let chunk = vec![b'a'; READ_ROOM];
    let mut chunks = vec![&chunk[..]; LINE_LIMIT / READ_ROOM - 1];
    chunks.push(&chunk[10..]);
    let last = [&[b'a'; 20][..], b"\n"].concat();
    chunks.push(&last);
    let (result, reads) = read_line_through(&chunks);
    assert_eq!(result, Err(TOO_LONG.to_owned()));
    let (last, before) = reads.split_last().unwrap();
    let before: usize = before.iter().sum();
    assert!(
        before <= LINE_LIMIT && before + last - 1 > LINE_LIMIT,
        "{reads:?}"
    );
}

/// 改行のないまま中身が `LINE_LIMIT` バイトを超えた行の連鎖は、種類 `Other` の誤りで終わる
/// (docs/spec/stdlib.md の「`Std.Net`」)。
#[test]
fn a_read_line_past_the_limit_fails() {
    let chunk = vec![b'a'; READ_ROOM];
    let mut chunks = vec![&chunk[..]; LINE_LIMIT / READ_ROOM];
    chunks.push(b"a");
    assert_eq!(read_line_through(&chunks).0, Err(TOO_LONG.to_owned()));
}

/// `read_line` が読みすぎたバイトは、次の読みが driver の操作を出さずに先に返す。受け付けの時点で終わった記録は鍵の
/// 項を持たず、待ちはすぐ再開する。残りを返しきったストリームは buffer の確保を放す (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_read_after_a_read_line_is_served_from_the_stream_buffer() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let scope = registry.lock().open_root(None, None).unwrap();
    let me = running_task(&registry, scope);
    let stream = net_stream(&mut ctx);
    let token = submit_read_line(
        &registry,
        &solo(&registry),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        stream,
    )
    .unwrap();
    let op = CellRef::decode(token);
    park_on(&registry, &mut ctx, &mut driver, token, me).unwrap();
    let mut out = Vec::new();
    let _ = read_line_round(&registry, &mut ctx, &mut driver, op, b"ab\ncd", &mut out);
    let stream = registry.lock().op_stream(token, 0).unwrap();
    let line = take_line(&registry, &mut ctx, token, 0).unwrap().unwrap();
    unsafe { ctx.heap_mut().decref(line) }.unwrap();
    let submitted = driver.submits;
    let buf = bytes_ab(&mut ctx, 0);
    let token = submit_read(
        &registry,
        &solo(&registry),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        buf,
        stream,
    )
    .unwrap();
    assert_eq!(driver.submits, submitted);
    assert_eq!(registry.lock().parts[0].in_flight, 0);
    let next = running_task(&registry, scope);
    let run = park_on(&registry, &mut ctx, &mut driver, token, next).unwrap();
    assert!(matches!(
        run,
        Some(Runnable::ResumeIo { cell, wake: WOKE, .. }) if cell == next
    ));
    let stream = registry.lock().op_stream(token, 0).unwrap();
    let back = take_bytes(&registry, &mut ctx, token, 0).unwrap();
    assert_eq!(unsafe { &*ctx.heap().bytes(back).unwrap() }, b"abcd");
    let s = net_rest(&ctx, stream);
    assert!(s.pending().is_empty() && s.rest.capacity() == 0);
    unsafe { ctx.heap_mut().decref(back) }.unwrap();
    unsafe { ctx.heap_mut().decref(stream) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 送った知らせを覚えるテストの宛先。
#[cfg(test)]
#[derive(Default)]
struct Posted {
    wakes: std::cell::RefCell<Vec<(u32, CellRef)>>,
    cancels: std::cell::RefCell<Vec<(u32, CellRef)>>,
    limits: std::cell::RefCell<Vec<(u32, CellRef)>>,
}

#[cfg(test)]
impl Post for Posted {
    fn wake(&self, k: u32, w: Woken) {
        let cell = match w {
            Woken::Task { cell, .. } => cell,
            Woken::Token { .. } => panic!("a token is sent to worker {k}"),
        };
        self.wakes.borrow_mut().push((k, cell));
    }

    fn cancel(&self, k: u32, cell: CellRef) {
        self.cancels.borrow_mut().push((k, cell));
    }

    fn limit(&self, k: u32, cell: CellRef) {
        self.limits.borrow_mut().push((k, cell));
    }

    fn place(&self, k: u32, _cell: CellRef, _worker: u32, _closure: Word) {
        panic!("a task is placed on worker {k}");
    }

    fn submit(&self, k: u32, _op: CellRef) {
        panic!("an op is submitted to worker {k}");
    }

    fn cancel_io(&self, k: u32, _op: CellRef) {
        panic!("an op cancel is sent to worker {k}");
    }

    fn completion(&self, k: u32, _op: CellRef) {
        panic!("a completion is sent to worker {k}");
    }
}

/// worker 0 が持ち主で、`Terminate` を待って止まっているタスクのセル。継続は即値で、誰も取り出さない。
#[cfg(test)]
pub(crate) fn terminate_waiter(shared: &Shared) -> CellRef {
    let scope = shared.lock().open_root(None, None).unwrap();
    let cell = running_task(shared, scope);
    let run = shared.lock().park_signal(1, cell, Word::UNIT).unwrap();
    assert!(run.is_none());
    cell
}

/// 登録表の lock を持ったままハンドルを落としても、破棄処理は mail の葉の lock だけを取るので止まらない
/// (docs/spec/vm.md の「セルと登録表」)。
#[test]
fn a_handle_dropped_under_the_registry_lock_does_not_deadlock() {
    let shared = Arc::new(Shared::new(Registry::new(Vec::new(), 1)));
    let mut ctx = test_ctx();
    let scope = shared.lock().open_root(None, None).unwrap();
    let cell = running_task(&shared, scope);
    let handle = task_handle(&mut ctx, &shared, cell);
    {
        let _cells = shared.lock();
        unsafe { ctx.heap_mut().decref(handle) }.unwrap();
    }
    assert!(matches!(shared.mail.take(0)[..], [MailItem::HandleDropped(c)] if c == cell));
}

/// 登録表の lock を持ったまま、host のスレッドでないスレッドでストリームを落としても、破棄処理は host の bin の葉の
/// lock だけを取るので止まらない。落としたハンドルは bin に積まれ、host を起こす印が立つ (docs/spec/vm.md の
/// 「セルと登録表」「操作の記録」)。
#[test]
fn a_stream_dropped_under_the_registry_lock_does_not_deadlock() {
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let mut ctx = test_ctx();
    // テストのスレッドを host でない worker とするため、host のスレッドが別にある bin を作る
    let bin = std::thread::spawn(|| Arc::new(HostBin::new(Arc::default(), false)))
        .join()
        .unwrap();
    let stream = stream_on(&mut ctx, 7, Arc::clone(&bin));
    {
        let _cells = shared.lock();
        unsafe { ctx.heap_mut().decref(stream) }.unwrap();
    }
    assert_eq!(*bin.bin.dropped.lock().unwrap(), [7]);
    assert!(bin.take_ring());
}

/// 持ち主が自分のタスクの取り消しは、落ちたハンドルとシグナルと同じ mail に積んだ順に出る。持ち主がほかの worker の
/// タスクの取り消しは、mail に積まずに持ち主の受け箱へ送る。ほかの worker は、持ち主の取り消しの項を順を保って残し、
/// ほかの項だけを取り出す (docs/spec/vm.md の「取り消しの届け方」)。
#[test]
fn mail_keeps_arrival_order() {
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let scope = shared.lock().open_root(None, None).unwrap();
    let (a, b) = (running_task(&shared, scope), running_task(&shared, scope));
    let posted = Posted::default();
    let here = |me| Here {
        me,
        host: me,
        mail: &shared.mail,
        post: &posted,
    };
    let mut discards = Discards::default();
    let order = |items: &[MailItem]| -> Vec<String> {
        items
            .iter()
            .map(|item| match *item {
                MailItem::HandleDropped(c) => format!("dropped {}", c.encode()),
                MailItem::Cancel { cell, owner } => format!("cancel {} of {owner}", cell.encode()),
                MailItem::Signal(bits) => format!("signal {bits}"),
                MailItem::Limit(c) => format!("limit {}", c.encode()),
            })
            .collect()
    };
    shared.mail.push(MailItem::HandleDropped(a));
    shared
        .lock()
        .let_go(&here(0), b, true, &mut discards)
        .unwrap();
    shared.mail.push(MailItem::Signal(Control::TERMINATE));
    let expected = [
        format!("dropped {}", a.encode()),
        format!("cancel {} of 0", b.encode()),
        format!("signal {}", Control::TERMINATE),
    ];
    assert_eq!(order(&shared.mail.take(0)), expected);
    assert!(!shared.mail.pending());
    // worker 1 が worker 0 のタスクに印を付けると、持ち主の受け箱へ送る
    shared
        .lock()
        .let_go(&here(1), a, true, &mut discards)
        .unwrap();
    assert!(!shared.mail.pending());
    assert_eq!(*posted.cancels.borrow(), [(0, a)]);
    // 同じ3つの項を積み直すと、worker 1 は取り消しの項を残してほかを取り出す
    shared.mail.push(MailItem::HandleDropped(a));
    shared.mail.push(MailItem::Cancel { cell: b, owner: 0 });
    shared.mail.push(MailItem::Signal(Control::TERMINATE));
    assert_eq!(
        order(&shared.mail.take(1)),
        [expected[0].clone(), expected[2].clone()]
    );
    assert!(shared.mail.pending());
    assert!(shared.mail.take(1).is_empty());
    assert_eq!(order(&shared.mail.take(0)), [expected[1].clone()]);
    assert!(!shared.mail.pending());
    assert!(posted.wakes.borrow().is_empty());
}

/// 持ち主 `owner` で、今から `ms` ミリ秒の時間を待って止まっているタスクのセル。継続は即値である。
#[cfg(test)]
pub(crate) fn sleeping_task(shared: &Shared, owner: u32, ms: i64) -> CellRef {
    let scope = shared.lock().open_root(None, None).unwrap();
    let cell = running_task(shared, scope);
    shared.lock().task_mut(cell).unwrap().owner = owner;
    let run = shared.lock().park_time(0, ms, cell, Word::UNIT).unwrap();
    assert!(run.is_none());
    cell
}

/// スコープ `scope` に属し、論理的な ID `worker` で動いている、worker 0 が持ち主のタスクのセル。`finish` で終えられる。
#[cfg(test)]
pub(crate) fn member_task(shared: &Shared, scope: CellRef, worker: u32) -> CellRef {
    let cell = running_task(shared, scope);
    let mut cells = shared.lock();
    let at = {
        let s = cells.scope_mut(scope).unwrap();
        s.members.push(cell);
        s.members.len() as u32 - 1
    };
    let task = cells.task_mut(cell).unwrap();
    task.member_at = at;
    task.worker = worker;
    cell
}

/// 持ち主が自分のタスクに付けた取り消しは mail で持ち主を待つ。ほかの worker の集める地点はそれを取らないので、
/// 持ち主の次の集める地点が待ちを外し、`Cancelled` で再開する。その後の満期には外したタイマーが残らず、起こしは
/// 重ならない (docs/spec/vm.md の「取り消しの届け方」)。
#[test]
fn a_cancel_in_the_mail_waits_for_its_owner() {
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let here = |me| Here {
        me,
        host: me,
        mail: &shared.mail,
        post: &Unposted,
    };
    let cell = sleeping_task(&shared, 0, 5);
    shared
        .lock()
        .let_go(&here(0), cell, true, &mut Discards::default())
        .unwrap();
    // worker 1 の集める地点は、worker 0 の取り消しを取らない
    let mut ctx = test_ctx();
    let mut out = Vec::new();
    assert!(
        !drain(
            &shared,
            &here(1),
            &mut ctx,
            None,
            0,
            &mut out,
            &mut Vec::new()
        )
        .unwrap()
    );
    assert!(out.is_empty() && shared.mail.pending());
    // 持ち主の次の集める地点が、満期より先に待ちを外す
    assert!(
        drain(
            &shared,
            &here(0),
            &mut ctx,
            None,
            0,
            &mut out,
            &mut Vec::new()
        )
        .unwrap()
    );
    assert_eq!(resumed_cells(&out), [(cell, CANCELLED)]);
    assert!(!shared.mail.pending());
    out.clear();
    expire(&shared, &mut NoDriver::default(), 0, 5, &mut out).unwrap();
    assert!(out.is_empty());
    assert!(shared.lock().task_mut(cell).unwrap().k.is_none());
}

/// 動いているタスクへの上限の旗は、持ち主がほかの worker なら持ち主へ `Notice::Limit` で回し、自分が持ち主ならセルを
/// 返して実行系に旗を付けさせる。どちらでもセルの旗は立つ (docs/spec/vm.md の「タスクの上限」)。
#[test]
fn a_limit_goes_to_the_owner_of_a_running_task() {
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let posted = Posted::default();
    let here = Here {
        me: 0,
        host: 0,
        mail: &shared.mail,
        post: &posted,
    };
    let elsewhere = owned_task(&shared, 1);
    assert_eq!(shared.lock().limit_task(elsewhere, &here).unwrap(), None);
    assert_eq!(*posted.limits.borrow(), [(1, elsewhere)]);
    assert!(shared.lock().task_mut(elsewhere).unwrap().limit);
    let mine = owned_task(&shared, 0);
    assert_eq!(shared.lock().limit_task(mine, &here).unwrap(), Some(mine));
    assert_eq!(posted.limits.borrow().len(), 1);
    assert!(shared.lock().task_mut(mine).unwrap().limit);
}

/// 待っているタスクと置いたタスクには旗だけが立ち、`resumed` と `started` が返す。起こした側の `clear_limit` で下りる。
/// 終わったセルと死んだセルには何もしない (docs/spec/vm.md の「タスクの上限」)。
#[test]
fn a_limit_on_a_waiting_task_waits_for_its_entry() {
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let parked = sleeping_task(&shared, 0, 5);
    assert_eq!(limit_task(&shared, parked), None);
    let (_, marks) = shared.lock().resumed(parked, WOKE, false).unwrap();
    assert_eq!(
        marks,
        TaskMarks {
            limit: true,
            bytes: 0
        }
    );
    shared.lock().clear_limit(parked).unwrap();
    assert!(!shared.lock().task_mut(parked).unwrap().limit);
    let placed = placed_task(&shared, 0);
    assert_eq!(limit_task(&shared, placed), None);
    let (_, marks) = shared.lock().started(placed).unwrap();
    assert_eq!(
        marks,
        TaskMarks {
            limit: true,
            bytes: 0
        }
    );
    let finished = owned_task(&shared, 0);
    shared.lock().task_mut(finished).unwrap().phase = Phase::Finished;
    assert_eq!(limit_task(&shared, finished), None);
    assert!(!shared.lock().task_mut(finished).unwrap().limit);
    let dead = owned_task(&shared, 0);
    shared.lock().release(dead.index);
    assert_eq!(limit_task(&shared, dead), None);
}

/// 回した `Notice::Limit` が届く前にタスクが `ResourceLimit` を起こして旗を下ろしていれば、持ち主は旗を立て直さず、
/// `Task` にも付けない。遅れた知らせで2度目の失敗を起こさないためである。旗がまだ立っていればセルを返す
/// (docs/spec/vm.md の「タスクの上限」)。
#[test]
fn a_late_limit_notice_after_the_raise_is_dropped() {
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let posted = Posted::default();
    let here = |me| Here {
        me,
        host: me,
        mail: &shared.mail,
        post: &posted,
    };
    let cell = owned_task(&shared, 1);
    assert_eq!(shared.lock().limit_task(cell, &here(0)).unwrap(), None);
    assert_eq!(shared.lock().limit_forwarded(cell, &here(1)), Some(cell));
    shared.lock().clear_limit(cell).unwrap();
    assert_eq!(shared.lock().limit_forwarded(cell, &here(1)), None);
    assert!(!shared.lock().task_mut(cell).unwrap().limit);
    assert_eq!(*posted.limits.borrow(), [(1, cell)]);
}

/// 回した `Notice::Limit` を受けた worker が今の持ち主でなければ、旗が立っている間は今の持ち主へ回し直す。待ちの間に
/// 救済で持ち主が変わり、また動き始めたタスクに旗を届けるためである (docs/spec/vm.md の「タスクの上限」)。
#[test]
fn a_forwarded_limit_follows_a_new_owner() {
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let posted = Posted::default();
    let here = Here {
        me: 0,
        host: 0,
        mail: &shared.mail,
        post: &posted,
    };
    let cell = owned_task(&shared, 1);
    shared.lock().task_mut(cell).unwrap().limit = true;
    assert_eq!(shared.lock().limit_forwarded(cell, &here), None);
    assert_eq!(*posted.limits.borrow(), [(1, cell)]);
    assert!(shared.lock().task_mut(cell).unwrap().limit);
}

/// mail の上限の旗の項はどの worker も取り出す。取り出した worker がタスクの持ち主なら、集める地点がセルを返す
/// (docs/spec/vm.md の「セルと登録表」)。
#[test]
fn a_limit_in_the_mail_is_taken_by_any_worker() {
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let cell = owned_task(&shared, 0);
    for me in [0, 1] {
        shared.mail.push(MailItem::Limit(cell));
        assert!(matches!(shared.mail.take(me)[..], [MailItem::Limit(c)] if c == cell));
    }
    shared.mail.push(MailItem::Limit(cell));
    let (mut ctx, mut out, mut limited) = (test_ctx(), Vec::new(), Vec::new());
    assert!(
        drain(
            &shared,
            &solo(&shared),
            &mut ctx,
            None,
            0,
            &mut out,
            &mut limited
        )
        .unwrap()
    );
    assert!(out.is_empty());
    assert_eq!(limited, [cell]);
    assert!(shared.lock().task_mut(cell).unwrap().limit);
}

/// 持ち主 `owner` で動いているタスクのセル。持ち主の違う場合をスレッドなしで作るためである。
#[cfg(test)]
fn owned_task(shared: &Shared, owner: u32) -> CellRef {
    let scope = shared.lock().open_root(None, None).unwrap();
    let cell = running_task(shared, scope);
    shared.lock().task_mut(cell).unwrap().owner = owner;
    cell
}

/// worker `owner` が持ち主で、論理的な ID が `worker` の、動いているタスクのセル。`cancel` なら取り消しの印を付ける。
#[cfg(test)]
pub(crate) fn task_on(shared: &Shared, owner: u32, worker: u32, cancel: bool) -> CellRef {
    let cell = owned_task(shared, owner);
    let mut cells = shared.lock();
    let task = cells.task_mut(cell).unwrap();
    task.worker = worker;
    task.cancel = cancel;
    cell
}

/// worker `owner` が持ち主の、`start` が置いてまだ始まっていないタスクのセル。論理的な ID は 0 である。
#[cfg(test)]
pub(crate) fn placed_task(shared: &Shared, owner: u32) -> CellRef {
    let cell = owned_task(shared, owner);
    shared.lock().task_mut(cell).unwrap().phase = Phase::Placed;
    cell
}

/// worker 0 の driver のハンドル `handle` の `TcpStream` の `Foreign`。落ちたら `bin` に積む。
#[cfg(test)]
pub(crate) fn stream_on<M: Mode>(ctx: &mut Ctx<M>, handle: IoHandle, bin: Arc<HostBin>) -> Word {
    let stream = IoStream {
        bind: Bind::Bound {
            host: 0,
            handle,
            bin,
        },
        origin: Origin::Peer(([127, 0, 0, 1], 1).into()),
        rest: Vec::new(),
        start: 0,
    };
    unsafe { ctx.heap_mut() }.alloc_foreign(stream, true)
}

/// 記録 `token` に host が置いた、まだ当てていない結果の、取り消しだけで終わったかの印。置いていなければ `None` である。
#[cfg(test)]
pub(crate) fn arrival_of(shared: &Shared, token: i64) -> Option<bool> {
    let mut cells = shared.lock();
    let record = cells.op_mut(CellRef::decode(token)).unwrap();
    record.arrival.as_ref().map(|&(_, cancelled)| cancelled)
}

/// タスク `cell` に取り消しの印を付ける。待ちは外さない。
#[cfg(test)]
pub(crate) fn flag_cancel(shared: &Shared, cell: CellRef) {
    shared.lock().task_mut(cell).unwrap().cancel = true;
}

/// worker 0 だけの run で、mail の上限の旗の項を受けたときと同じくタスク `cell` に旗を立てる。旗を付ける VM の `Task` が
/// worker 0 にあればセルを返す。
#[cfg(test)]
pub(crate) fn limit_task(shared: &Shared, cell: CellRef) -> Option<CellRef> {
    shared.lock().limit_task(cell, &solo(shared)).unwrap()
}

/// タスク `cell` に上限の旗が立っているか。
#[cfg(test)]
pub(crate) fn limit_flag(shared: &Shared, cell: CellRef) -> bool {
    shared.lock().task_mut(cell).unwrap().limit
}

/// 記録 `token` が、`main` の `Finish` で残ったタスクとみなす状態 (driver に出したままか置いたまま) か。
#[cfg(test)]
pub(crate) fn op_left(shared: &Shared, token: i64) -> bool {
    let mut cells = shared.lock();
    let record = cells.op_mut(CellRef::decode(token)).unwrap();
    record.submitted || record.pending_op.is_some()
}

/// 預かる ID を開かない channel。品物は即値だけを使い、預かり先を閉じないテストで使う。
#[cfg(test)]
pub(crate) fn test_chan(shared: &Shared, cap: i64) -> i64 {
    shared.lock().new_chan(cap, Depot::of(0))
}

/// ほかの worker の送りを表す。予約して、同じ worker が即値 `n` を積んで確定する。起こした待ち手を返す。
#[cfg(test)]
pub(crate) fn send_now(shared: &Shared, c: i64, n: i64) -> Vec<Woken> {
    let mut hold = None;
    assert_eq!(
        shared
            .lock()
            .reserve_send(c, None, false, &mut hold)
            .unwrap(),
        0
    );
    let (chan, _) = Hold::take_send(&mut hold, c).unwrap();
    let mut out = Vec::new();
    shared
        .lock()
        .push_raw(chan, Word::imm(n), &mut out)
        .unwrap();
    out
}

/// 予約した送りを、即値 `n` を積んで確定する。起こした待ち手を返す。
#[cfg(test)]
fn push_held(shared: &Shared, hold: &mut Option<Hold>, c: i64, n: i64) -> Vec<Woken> {
    let (chan, _) = Hold::take_send(hold, c).unwrap();
    let mut out = Vec::new();
    shared
        .lock()
        .push_raw(chan, Word::imm(n), &mut out)
        .unwrap();
    out
}

/// タスク `me` を channel `c` の受け手として待たせる。
#[cfg(test)]
pub(crate) fn park_receiver(shared: &Shared, c: i64, me: CellRef) {
    let mut out = Vec::new();
    let run = shared
        .lock()
        .park_recv(c, me, Word::UNIT, &mut out)
        .unwrap();
    assert!(run.is_none() && out.is_empty());
}

#[cfg(test)]
fn reserve_send_on(shared: &Shared, c: i64) -> i64 {
    shared
        .lock()
        .reserve_send(c, None, false, &mut None)
        .unwrap()
}

/// 持ち主 `me` が取り消しの印の付いたタスク `cell` の待ちを外し、再開する仕事を返す。
#[cfg(test)]
fn cancel_on(shared: &Shared, cell: CellRef, me: u32) -> Vec<Runnable> {
    let mut cells = shared.lock();
    cells.task_mut(cell).unwrap().cancel = true;
    let mut woken = Vec::new();
    let mut work = IoWork::default();
    let here = Here {
        me,
        host: me,
        mail: &shared.mail,
        post: &Unposted,
    };
    cells.unpark(cell, &here, &mut woken, &mut work).unwrap();
    let mut out = Vec::new();
    cells.route(&mut woken, &here, &mut out).unwrap();
    out
}

#[cfg(test)]
fn woken_cells(out: &[Woken]) -> Vec<CellRef> {
    out.iter()
        .map(|w| match *w {
            Woken::Task { cell, wake, op } => {
                assert!(wake == WOKE && op.is_none());
                cell
            }
            Woken::Token { .. } => panic!("expected a task"),
        })
        .collect()
}

#[cfg(test)]
fn chan_state(shared: &Shared, c: i64) -> (usize, usize, usize, usize) {
    let mut cells = shared.lock();
    let chan = cells.chan_mut(CellRef::decode(c)).unwrap();
    (
        chan.items.len(),
        chan.sends,
        chan.recvs,
        chan.receivers.len(),
    )
}

/// 予約した送りは確定するまで枠を取っておき、ほかの送りはその枠を使えない (docs/spec/vm.md の「channel」)。
#[test]
fn a_reserved_send_holds_its_room_until_the_push() {
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let c = test_chan(&shared, 1);
    let mut held = None;
    assert_eq!(
        shared
            .lock()
            .reserve_send(c, None, false, &mut held)
            .unwrap(),
        0
    );
    assert!(matches!(held, Some(Hold::Send { chan, .. }) if chan == CellRef::decode(c)));
    let mut other = None;
    assert_eq!(
        shared
            .lock()
            .reserve_send(c, None, false, &mut other)
            .unwrap(),
        1
    );
    assert!(other.is_none());
    assert!(push_held(&shared, &mut held, c, 7).is_empty());
    assert_eq!(reserve_send_on(&shared, c), 1);
    let mut taken = None;
    assert_eq!(
        shared
            .lock()
            .reserve_recv(c, None, false, &mut taken, &mut Vec::new())
            .unwrap(),
        0
    );
    assert!(matches!(taken, Some(Hold::Recv { item, .. }) if item == Word::imm(7)));
    assert_eq!(shared.lock().stats.channel_ops, 1);
}

/// 受け取りの予約は先頭の品物を列から取り出し、確定するまで取り出した数に数える。
#[test]
fn a_reserved_receive_takes_the_item_out_of_the_queue() {
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let c = test_chan(&shared, 2);
    send_now(&shared, c, 1);
    send_now(&shared, c, 2);
    let mut held = None;
    assert_eq!(
        shared
            .lock()
            .reserve_recv(c, None, false, &mut held, &mut Vec::new())
            .unwrap(),
        0
    );
    assert!(matches!(held, Some(Hold::Recv { item, .. }) if item == Word::imm(1)));
    assert_eq!(chan_state(&shared, c), (1, 0, 1, 0));
    let (chan, _, item) = Hold::take_recv(&mut held, c).unwrap();
    assert!(item == Word::imm(1));
    assert!(shared.lock().pop_raw(chan).unwrap().is_none());
    assert_eq!(chan_state(&shared, c), (1, 0, 0, 0));
}

/// 予約のない確定は、標準ライブラリが予約と確定の組を崩したときだけ起きるので、内部の誤りにする。
#[test]
fn a_commit_without_a_reservation_is_an_internal_error() {
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let c = test_chan(&shared, 1);
    let chan = CellRef::decode(c);
    let pushed = shared.lock().push_raw(chan, Word::imm(1), &mut Vec::new());
    assert!(matches!(
        pushed,
        Err(CellFault::Internal(
            "an item is pushed without a reserved send"
        ))
    ));
    assert!(matches!(
        shared.lock().pop_raw(chan),
        Err(CellFault::Internal(
            "an item is popped without a reserved receive"
        ))
    ));
    assert!(matches!(
        Hold::take_send(&mut None, c),
        Err(CellFault::Internal(
            "an item is pushed without a reserved send"
        ))
    ));
    assert!(matches!(
        Hold::take_recv(&mut None, c),
        Err(CellFault::Internal(
            "an item is popped without a reserved receive"
        ))
    ));
}

/// 閉じる前に予約した送りは閉じた後に積まれ、受け手はそれを待って受け取る。受け取った後は閉じて空になる
/// (docs/spec/stdlib.md の「`Std.Chan`」)。
#[test]
fn a_close_before_a_reserved_push_keeps_receivers_waiting_for_the_item() {
    let mut ctx = test_ctx();
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let c = shared.lock().new_chan(1, Depot::open(&mut ctx));
    let mut held = None;
    assert_eq!(
        shared
            .lock()
            .reserve_send(c, None, false, &mut held)
            .unwrap(),
        0
    );
    let mut out = Vec::new();
    assert!(shared.lock().close_raw(c, &mut out).unwrap().is_none());
    assert!(out.is_empty());
    let mut taken = None;
    let reserve_recv = |taken: &mut Option<Hold>| {
        shared
            .lock()
            .reserve_recv(c, None, false, taken, &mut Vec::new())
            .unwrap()
    };
    assert_eq!(reserve_recv(&mut taken), 1);
    assert!(push_held(&shared, &mut held, c, 5).is_empty());
    assert_eq!(reserve_recv(&mut taken), 0);
    let (chan, _, _) = Hold::take_recv(&mut taken, c).unwrap();
    let depot = shared.lock().pop_raw(chan).unwrap().unwrap();
    depot.close(&mut ctx).unwrap();
    assert_eq!(reserve_recv(&mut taken), 2);
}

/// 閉じた channel に予約した送りが残る間は受け手が待ち、最後の送りが積まれると、待っている受け手をすべて起こす。
/// 起こしは持ち主によらず、宛先は受け取る worker の `take_wake` が決める。
#[test]
fn the_last_reserved_push_into_a_closed_channel_wakes_every_receiver() {
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let c = test_chan(&shared, 2);
    let (mut a, mut b) = (None, None);
    assert_eq!(
        shared.lock().reserve_send(c, None, false, &mut a).unwrap(),
        0
    );
    assert_eq!(
        shared.lock().reserve_send(c, None, false, &mut b).unwrap(),
        0
    );
    assert!(
        shared
            .lock()
            .close_raw(c, &mut Vec::new())
            .unwrap()
            .is_none()
    );
    let r1 = owned_task(&shared, 0);
    let (r2, r3) = (owned_task(&shared, 1), owned_task(&shared, 1));
    for r in [r1, r2, r3] {
        park_receiver(&shared, c, r);
    }
    assert_eq!(woken_cells(&push_held(&shared, &mut a, c, 1)), [r1]);
    assert_eq!(woken_cells(&push_held(&shared, &mut b, c, 2)), [r2, r3]);
    for r in [r2, r3] {
        assert_eq!(shared.lock().task_mut(r).unwrap().owner, 1);
    }
    assert_eq!(chan_state(&shared, c), (2, 0, 0, 0));
}

/// 受け取りの予約が残る間は、閉じて空でも channel を放さない。確定した区間で放し、預かり先を返す。
#[test]
fn a_channel_is_released_only_after_the_last_reserved_receive() {
    let mut ctx = test_ctx();
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let c = shared.lock().new_chan(1, Depot::open(&mut ctx));
    send_now(&shared, c, 1);
    let mut held = None;
    assert_eq!(
        shared
            .lock()
            .reserve_recv(c, None, false, &mut held, &mut Vec::new())
            .unwrap(),
        0
    );
    assert!(
        shared
            .lock()
            .close_raw(c, &mut Vec::new())
            .unwrap()
            .is_none()
    );
    assert_eq!(chan_state(&shared, c), (0, 0, 1, 0));
    let (chan, _, _) = Hold::take_recv(&mut held, c).unwrap();
    let depot = shared.lock().pop_raw(chan).unwrap().unwrap();
    depot.close(&mut ctx).unwrap();
    assert!(shared.lock().chan_mut(chan).is_none());
}

/// ほかの worker が持ち主の受け手を起こすと、待ちの登録だけを外し、継続はセルに残す。継続を取り出すのは持ち主の
/// `take_wake` だけで、ほかの worker は持ち主へ回す (docs/spec/vm.md の「セルと登録表」)。
#[test]
fn a_wake_for_another_owner_leaves_the_continuation_in_the_cell() {
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let c = test_chan(&shared, 0);
    let r = owned_task(&shared, 1);
    park_receiver(&shared, c, r);
    let out = send_now(&shared, c, 4);
    assert_eq!(woken_cells(&out), [r]);
    {
        let mut cells = shared.lock();
        let task = cells.task_mut(r).unwrap();
        assert!(matches!(task.phase, Phase::Parked));
        assert!(task.wait.is_none() && task.k.is_some());
    }
    let w = &out[0];
    assert!(matches!(
        shared.lock().take_wake(w, 0).unwrap(),
        Route::Forward(1)
    ));
    assert!(matches!(
        shared.lock().take_wake(w, 1).unwrap(),
        Route::Run(Runnable::Resume { cell, wake: WOKE, .. }) if cell == r
    ));
}

/// 起こされた後に届いた取り消しは待ちを外さない。待ちの登録を先に下ろした側が勝つので、受け手は `Woke` で再開して
/// 速い道をやり直す (docs/spec/vm.md の「取り消しの届け方」)。
#[test]
fn a_cancel_after_the_wake_leaves_the_task_woken() {
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let c = test_chan(&shared, 0);
    let r = owned_task(&shared, 1);
    park_receiver(&shared, c, r);
    let out = send_now(&shared, c, 4);
    let cancels = shared.lock().stats.cancels;
    assert!(cancel_on(&shared, r, 1).is_empty());
    assert_eq!(shared.lock().stats.cancels, cancels);
    assert!(matches!(
        shared.lock().take_wake(&out[0], 1).unwrap(),
        Route::Run(Runnable::Resume { cell, wake: WOKE, .. }) if cell == r
    ));
}

/// 起こされる前の取り消しは受け手を列から外すので、容量 0 の channel の枠も閉じる。
#[test]
fn a_cancel_before_the_wake_takes_the_receiver_out_of_the_queue() {
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let c = test_chan(&shared, 0);
    let r = owned_task(&shared, 0);
    park_receiver(&shared, c, r);
    assert_eq!(resumed_cells(&cancel_on(&shared, r, 0)), [(r, CANCELLED)]);
    assert_eq!(chan_state(&shared, c), (0, 0, 0, 0));
    assert_eq!(reserve_send_on(&shared, c), 1);
}

/// 待っている受け手が開いた枠を送りが予約した後に、その受け手が取り消されても、品物は積まれる。容量によらず、次の
/// 受け手が取るまで、channel は容量と待っている受け手の数の和を超えて持ち、ほかの送りは積めない。ここでは容量 0 で
/// 確かめる (docs/spec/stdlib.md の「`Std.Chan`」)。
#[test]
fn receivers_cancelled_after_sends_reserved_their_room_overfill_the_channel_until_the_next_receive()
{
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let c = test_chan(&shared, 0);
    let r = owned_task(&shared, 0);
    park_receiver(&shared, c, r);
    let mut held = None;
    assert_eq!(
        shared
            .lock()
            .reserve_send(c, None, false, &mut held)
            .unwrap(),
        0
    );
    assert_eq!(resumed_cells(&cancel_on(&shared, r, 0)), [(r, CANCELLED)]);
    assert!(push_held(&shared, &mut held, c, 9).is_empty());
    assert_eq!(chan_state(&shared, c), (1, 0, 0, 0));
    assert_eq!(reserve_send_on(&shared, c), 1);
    let mut taken = None;
    assert_eq!(
        shared
            .lock()
            .reserve_recv(c, None, false, &mut taken, &mut Vec::new())
            .unwrap(),
        0
    );
    let (chan, _, item) = Hold::take_recv(&mut taken, c).unwrap();
    assert!(item == Word::imm(9));
    assert!(shared.lock().pop_raw(chan).unwrap().is_none());
    assert_eq!(reserve_send_on(&shared, c), 1);
    park_receiver(&shared, c, owned_task(&shared, 0));
    assert_eq!(reserve_send_on(&shared, c), 0);
}

/// 速い道が待つと答えた後に、ほかの worker が品物を積むと、`park_recv` は登録せずに `Woke` で戻る。待ちには数える。
#[test]
fn park_returns_at_once_when_another_worker_changed_the_channel() {
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let c = test_chan(&shared, 1);
    let me = owned_task(&shared, 0);
    assert_eq!(
        shared
            .lock()
            .reserve_recv(c, Some(me), false, &mut None, &mut Vec::new())
            .unwrap(),
        1
    );
    assert!(send_now(&shared, c, 3).is_empty());
    let mut out = Vec::new();
    let run = shared
        .lock()
        .park_recv(c, me, Word::UNIT, &mut out)
        .unwrap();
    assert_eq!(resumed_cells(&Vec::from_iter(run)), [(me, WOKE)]);
    assert!(out.is_empty());
    assert_eq!(shared.lock().stats.channel_waits, 1);
    assert_eq!(chan_state(&shared, c), (1, 0, 0, 0));
}

/// タスク `cell` の、まだ `Finish` していないエントリの `Task` の数。
#[cfg(test)]
pub(crate) fn entries_of(shared: &Shared, cell: CellRef) -> u32 {
    shared.lock().task_mut(cell).unwrap().entries
}

/// タスク `cell` の今の持ち主。
#[cfg(test)]
pub(crate) fn owner_of(shared: &Shared, cell: CellRef) -> u32 {
    shared.lock().task_mut(cell).unwrap().owner
}

/// `entries` は始めるときと再開するときに1つ増え、待ちに入ったエントリの `Task` の後始末 (`leave`) で1つ減る。
/// すぐ戻る待ちでは減らない。0 のセルを減らすのは内部の誤りである (docs/spec/vm.md の「救済」)。
#[test]
fn an_entry_count_follows_start_park_resume_and_finish() {
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let cell = placed_task(&shared, 0);
    shared.lock().started(cell).unwrap();
    assert_eq!(entries_of(&shared, cell), 1);
    assert!(
        shared
            .lock()
            .park_time(0, 5, cell, Word::UNIT)
            .unwrap()
            .is_none()
    );
    assert_eq!(entries_of(&shared, cell), 1);
    assert_eq!(shared.lock().leave(&mut vec![(cell, 0)]).unwrap(), 1);
    assert_eq!(entries_of(&shared, cell), 0);
    let mut out = Vec::new();
    expire(&shared, &mut NoDriver::default(), 0, 5, &mut out).unwrap();
    assert_eq!(resumed_cells(&out), [(cell, WOKE)]);
    shared.lock().resumed(cell, WOKE, false).unwrap();
    assert_eq!(entries_of(&shared, cell), 1);
    // 譲りはすぐ戻る仕事を返し、前のエントリの `Task` はまだ終わっていない
    let run = shared.lock().ready(cell, Word::UNIT).unwrap();
    assert!(matches!(run, Runnable::Resume { cell: c, .. } if c == cell));
    assert_eq!(entries_of(&shared, cell), 1);
    let mut left = vec![(cell, 0)];
    assert_eq!(shared.lock().leave(&mut left).unwrap(), 1);
    assert!(left.is_empty());
    shared.lock().resumed(cell, WOKE, false).unwrap();
    assert_eq!(entries_of(&shared, cell), 1);
    assert_eq!(shared.lock().leave(&mut vec![(cell, 0)]).unwrap(), 1);
    assert!(matches!(
        shared.lock().leave(&mut vec![(cell, 0)]),
        Err(CellFault::Internal(_))
    ));
}

/// すぐ再開する待ちでは、前のエントリの `Task` が残る間に次のエントリの `Task` が動き始める。2つとも後始末を終えるまで、
/// 待ったタスクの起こしは盗めない (docs/spec/vm.md の「救済」)。
#[test]
fn two_live_entries_keep_a_parked_task_unstealable() {
    let shared = Shared::new(Registry::new(Vec::new(), 2));
    let cell = placed_task(&shared, 0);
    shared.lock().started(cell).unwrap();
    let run = shared.lock().ready(cell, Word::UNIT).unwrap();
    assert!(matches!(run, Runnable::Resume { .. }));
    shared.lock().resumed(cell, WOKE, false).unwrap();
    assert_eq!(entries_of(&shared, cell), 2);
    let c = test_chan(&shared, 0);
    park_receiver(&shared, c, cell);
    assert_eq!(shared.lock().leave(&mut vec![(cell, 0)]).unwrap(), 0);
    assert_eq!(entries_of(&shared, cell), 1);
    assert_eq!(send_now(&shared, c, 4).len(), 1);
    assert!(shared.lock().steal_wake(cell, 0, 1).unwrap().is_none());
    assert_eq!(shared.lock().task_mut(cell).unwrap().owner, 0);
    assert_eq!(shared.lock().leave(&mut vec![(cell, 0)]).unwrap(), 1);
    assert!(matches!(
        shared.lock().steal_wake(cell, 0, 1).unwrap(),
        Some((k, 0)) if k == Word::UNIT
    ));
    let mut cells = shared.lock();
    let task = cells.task_mut(cell).unwrap();
    assert_eq!(task.owner, 1);
    assert!(task.k.is_none());
}

/// 救済の helper は、持ち主が元の worker のセルだけを引き取る。ほかの worker のセルは、引き取れる状態でも持ち主を
/// 変えない (docs/spec/vm.md の「救済」)。
#[test]
fn steal_helpers_refuse_a_cell_owned_by_a_third_worker() {
    let shared = Shared::new(Registry::new(Vec::new(), 3));
    let placed = placed_task(&shared, 2);
    let yielded = owned_task(&shared, 2);
    shared.lock().ready(yielded, Word::UNIT).unwrap();
    let woken = owned_task(&shared, 2);
    let c = test_chan(&shared, 0);
    park_receiver(&shared, c, woken);
    assert_eq!(send_now(&shared, c, 4).len(), 1);
    let mut cells = shared.lock();
    assert!(!cells.steal_start(placed, 0, 1).unwrap());
    assert!(!cells.steal_resume(yielded, 0, 1).unwrap());
    assert!(cells.steal_wake(woken, 0, 1).unwrap().is_none());
    for cell in [placed, yielded, woken] {
        assert_eq!(cells.task_mut(cell).unwrap().owner, 2);
    }
    assert!(cells.task_mut(woken).unwrap().k.is_some());
    // 元の worker が持ち主なら、同じ3つを引き取る
    assert!(cells.steal_start(placed, 2, 1).unwrap());
    assert!(cells.steal_resume(yielded, 2, 1).unwrap());
    assert!(cells.steal_wake(woken, 2, 1).unwrap().is_some());
    for cell in [placed, yielded, woken] {
        assert_eq!(cells.task_mut(cell).unwrap().owner, 1);
    }
}

/// 後始末の前に放したタスクのセルは、`leave` が飛ばして数えない。生きているが種類の違うセルは内部の誤りである
/// (docs/spec/vm.md の「救済」)。
#[test]
fn leave_skips_a_released_cell() {
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let cell = placed_task(&shared, 0);
    shared.lock().started(cell).unwrap();
    shared.lock().release(cell.index);
    // 放した添字は次のセル (ここでは次のタスクの根のスコープ) が使うが、古い番号は generation で見分ける
    let next = placed_task(&shared, 0);
    assert!(shared.lock().scope_cell(cell.encode()).is_none());
    shared.lock().started(next).unwrap();
    let mut left = vec![(cell, 0)];
    assert_eq!(shared.lock().leave(&mut left).unwrap(), 0);
    assert!(left.is_empty());
    assert_eq!(entries_of(&shared, next), 1);
    let scope = shared.lock().open_root(None, None).unwrap();
    assert!(matches!(
        shared.lock().leave(&mut vec![(scope, 0)]),
        Err(CellFault::Internal(_))
    ));
}

/// 待ちに入って `Finish` したエントリの数は `leave` がセルに足し、次の `started` か `resumed` が返す。数は待ちをまたいで
/// 足し続け、放したセルの分は捨てる (docs/spec/vm.md の「セルと登録表」)。
#[test]
fn leave_adds_the_bytes_and_resumed_returns_them() {
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let cell = placed_task(&shared, 0);
    let (_, marks) = shared.lock().started(cell).unwrap();
    assert_eq!(marks.bytes, 0);
    for (now, added, total) in [(0, 100, 100), (5, 50, 150)] {
        assert!(
            shared
                .lock()
                .park_time(now, 5, cell, Word::UNIT)
                .unwrap()
                .is_none()
        );
        assert_eq!(shared.lock().leave(&mut vec![(cell, added)]).unwrap(), 1);
        let mut out = Vec::new();
        expire(&shared, &mut NoDriver::default(), 0, now + 5, &mut out).unwrap();
        assert_eq!(resumed_cells(&out), [(cell, WOKE)]);
        let (_, marks) = shared.lock().resumed(cell, WOKE, false).unwrap();
        assert_eq!(marks.bytes, total);
    }
    let gone = placed_task(&shared, 0);
    shared.lock().started(gone).unwrap();
    shared.lock().release(gone.index);
    assert_eq!(shared.lock().leave(&mut vec![(gone, 7)]).unwrap(), 0);
}

/// 旗で `ResourceLimit` を起こした実行系の `clear_limit` は、旗と一緒にタスクの数も 0 に戻す。次のエントリは 0 から
/// 数える (docs/spec/vm.md の「タスクの上限」)。
#[test]
fn clear_limit_zeroes_the_bytes() {
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let parked = sleeping_task(&shared, 0, 5);
    shared.lock().task_mut(parked).unwrap().bytes = 4096;
    assert_eq!(limit_task(&shared, parked), None);
    let (_, marks) = shared.lock().resumed(parked, WOKE, false).unwrap();
    assert_eq!(
        marks,
        TaskMarks {
            limit: true,
            bytes: 4096
        }
    );
    shared.lock().clear_limit(parked).unwrap();
    let mut cells = shared.lock();
    let task = cells.task_mut(parked).unwrap();
    assert_eq!((task.limit, task.bytes), (false, 0));
}

/// 起動しない子のプロセス。`kill` と `wait` の回数を数え、`wait` は `code` を返す。`raw` が立っていれば、driver が
/// 終わりを待つ OS の子のふりをする。
#[cfg(test)]
struct FakeProcess {
    raw: bool,
    code: i32,
    kills: Arc<std::sync::atomic::AtomicUsize>,
    waits: Arc<std::sync::atomic::AtomicUsize>,
}

#[cfg(test)]
impl Process for FakeProcess {
    fn wait(&mut self) -> io::Result<i32> {
        self.waits.fetch_add(1, Ordering::Relaxed);
        Ok(self.code)
    }

    fn kill(&mut self) -> io::Result<()> {
        self.kills.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    // テストの driver は受け付けた `Op::Wait` の中身を見ないので、Windows のハンドルは、この crate の `Cargo.toml` を
    // 開いたもので代える
    fn raw(&self) -> Option<eml_rt::RawProcess> {
        if !self.raw {
            return None;
        }
        #[cfg(unix)]
        let raw = eml_rt::RawProcess(4242);
        #[cfg(windows)]
        let raw = eml_rt::RawProcess(
            std::fs::File::open(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
                .unwrap()
                .into(),
        );
        Some(raw)
    }
}

/// 起動しない子の `kill` と `wait` の回数。
#[cfg(test)]
type Counts = Arc<std::sync::atomic::AtomicUsize>;

/// 段のプログラムが `programs` の、起動しない子の `Std.Exec.Child`。段 `k` の終了コードは `k + 3` である。kill と
/// wait の回数を数える表を添えて返す。
#[cfg(test)]
fn fake_child<M: Mode>(ctx: &mut Ctx<M>, programs: &[&str], raw: bool) -> (Word, Counts, Counts) {
    let (kills, waits): (Counts, Counts) = (Arc::default(), Arc::default());
    let processes = (0..programs.len())
        .map(|k| {
            Box::new(FakeProcess {
                raw,
                code: k as i32 + 3,
                kills: Arc::clone(&kills),
                waits: Arc::clone(&waits),
            }) as Box<dyn Process>
        })
        .collect();
    let child = ExecChild::new(
        programs.iter().map(|&p| p.into()).collect(),
        processes,
        None,
        None,
        None,
    );
    let w = unsafe { ctx.heap_mut() }.alloc_foreign(child, true);
    (w, kills, waits)
}

/// 子の終わりの待ちの記録は、持ち主の worker が host で、その driver に `Op::Wait` を出す。I/O をほかの worker に
/// 任せる worker (仮想の時計の worker 1 から先) の待ちは、内部の誤りである。待ちは回収しないので、知らせの後の
/// `reap` が段を回収して終了コードを返す (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_wait_record_is_hosted_by_its_owner() {
    let mut ctx = test_ctx();
    let shared = Shared::new(Registry::new(Vec::new(), 3));
    let mut driver = NoDriver::default();
    let here = Here {
        me: 2,
        host: 2,
        mail: &shared.mail,
        post: &Unposted,
    };
    let (child, _, waits) = fake_child(&mut ctx, &["eml"], true);
    let token = submit_wait(&shared, &here, &mut ctx, Some(&mut driver), 0, 0, 0, child).unwrap();
    let op = CellRef::decode(token);
    assert_eq!(driver.submits, 1);
    {
        let mut cells = shared.lock();
        let record = cells.op_mut(op).unwrap();
        assert_eq!((record.owner, record.host), (2, 2));
        assert!(matches!(record.phase, OpPhase::Transfer));
    }
    let arrival = shared
        .lock()
        .arrive(op, Ok(Done::Size(0)), false, 2)
        .unwrap();
    assert!(matches!(arrival, Arrival::Settle));
    let bin = shared.lock().host_bin(2, Arc::default()).unwrap();
    shared
        .lock()
        .settle_op(&mut ctx, &bin, 0, op, &here, &mut IoWork::default())
        .unwrap();
    assert_eq!(shared.lock().op_outcome(token, 0).unwrap(), OUTCOME_OK);
    let child = shared.lock().op_stream(token, 0).unwrap();
    take_op(&shared, &mut ctx, token, 0).unwrap();
    assert_eq!(waits.load(Ordering::Relaxed), 0);
    assert_eq!(exec_reap(&ctx, 0, child).unwrap().unwrap(), 3);
    assert_eq!(waits.load(Ordering::Relaxed), 1);
    // 回収した段は、破棄処理が待ち直さない
    unsafe { ctx.heap_mut().decref(child) }.unwrap();
    assert_eq!(waits.load(Ordering::Relaxed), 1);
    let elsewhere = Here {
        me: 1,
        host: 0,
        mail: &shared.mail,
        post: &Unposted,
    };
    let (child, _, _) = fake_child(&mut ctx, &["eml"], true);
    assert!(matches!(
        submit_wait(&shared, &elsewhere, &mut ctx, None, 0, 0, 0, child),
        Err(CellFault::Internal(
            "a wait is submitted on a worker that hosts no I/O"
        ))
    ));
    unsafe { ctx.heap_mut().decref(child) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// OS の子を持たない Host の段の待ちは、driver に出さずに受け付けで終わり、`reap` がその場で Host の `wait` を呼ぶ。
/// 回収した段は、もう一度回収しても同じ終了コードを返す (docs/spec/stdlib.md の「`Std.Exec`」)。
#[test]
fn a_wait_on_a_host_without_processes_is_done_at_submit() {
    let mut ctx = test_ctx();
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let mut driver = NoDriver::default();
    let (child, kills, waits) = fake_child(&mut ctx, &["first", "second"], false);
    let token = submit_wait(
        &shared,
        &solo(&shared),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        1,
        child,
    )
    .unwrap();
    assert_eq!(driver.submits, 0);
    assert_eq!(shared.lock().op_outcome(token, 0).unwrap(), OUTCOME_OK);
    let child = shared.lock().op_stream(token, 0).unwrap();
    take_op(&shared, &mut ctx, token, 0).unwrap();
    assert_eq!(waits.load(Ordering::Relaxed), 0);
    assert_eq!(exec_reap(&ctx, 1, child).unwrap().unwrap(), 4);
    assert_eq!(exec_reap(&ctx, 1, child).unwrap().unwrap(), 4);
    assert_eq!(waits.load(Ordering::Relaxed), 1);
    // 破棄処理は、回収していない段 0 だけを kill して待つ
    unsafe { ctx.heap_mut().decref(child) }.unwrap();
    assert_eq!(
        (kills.load(Ordering::Relaxed), waits.load(Ordering::Relaxed)),
        (1, 2)
    );
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 子を落とすと、すべての段を kill してから待つ (docs/spec/stdlib.md の「`Std.Exec`」)。
#[test]
fn a_dropped_exec_child_kills_and_waits_every_stage() {
    let mut ctx = test_ctx();
    let (child, kills, waits) = fake_child(&mut ctx, &["a", "b", "c"], true);
    assert_eq!(exec_stage_count(&ctx, child).unwrap(), 3);
    unsafe { ctx.heap_mut().decref(child) }.unwrap();
    assert_eq!(
        (kills.load(Ordering::Relaxed), waits.load(Ordering::Relaxed)),
        (3, 3)
    );
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 読み手が終わった子の pipe への書きの `BrokenPipe` は、ソケットの切断と同じ結果の番号 2 で、誤りの文言の前の語は
/// 先頭の段のプログラムを名指す。取り出した端と pipe にしなかった端は取り出せない (docs/spec/stdlib.md の
/// 「`Std.Exec`」)。
#[test]
fn a_pipe_write_broken_pipe_is_outcome_reset() {
    let mut ctx = test_ctx();
    let shared = Shared::new(Registry::new(Vec::new(), 1));
    let mut driver = NoDriver::default();
    let (child, _, _) = fake_child(&mut ctx, &["eml", "tail"], true);
    unsafe { &mut *ctx.heap().foreign_unique::<ExecChild>(child).unwrap() }.stdin =
        Some(Bind::Bound {
            host: 0,
            handle: 7,
            bin: Arc::default(),
        });
    let writer = exec_take(&mut ctx, child, ChildPipe::Stdin)
        .unwrap()
        .unwrap();
    assert_eq!(
        exec_take(&mut ctx, child, ChildPipe::Stdin).unwrap(),
        Err("cannot take the standard input of `eml`: not available".to_string())
    );
    assert_eq!(
        exec_take(&mut ctx, child, ChildPipe::Stdout).unwrap(),
        Err("cannot take the output of `tail`: not available".to_string())
    );
    let buf = unsafe { ctx.heap_mut() }.alloc_bytes(b"late");
    let token = submit_write(
        &shared,
        &solo(&shared),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        buf,
        writer,
    )
    .unwrap();
    let mut out = Vec::new();
    complete_op(
        &shared,
        &mut ctx,
        &mut driver,
        CellRef::decode(token),
        Err(io::ErrorKind::BrokenPipe.into()),
        &mut out,
    );
    assert_eq!(shared.lock().op_outcome(token, 0).unwrap(), OUTCOME_RESET);
    assert_eq!(
        shared.lock().op_prefix(token, 0).unwrap(),
        "cannot write to `eml`"
    );
    let taken = take_op(&shared, &mut ctx, token, 0).unwrap();
    assert!(taken.failure().is_none());
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    unsafe { ctx.heap_mut().decref(child) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 子の pipe の読みはファイルと同じく、空きの少ない buffer でも読めるようになるのを待たず、受け付けで広げて貸す。
/// 読みの誤りの文言は、最後の段の出力を名指す (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_read_of_a_pipe_grows_at_submit() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let buf = bytes_ab(&mut ctx, 0);
    let stream = IoStream {
        bind: Bind::Bound {
            host: 0,
            handle: 0,
            bin: Arc::default(),
        },
        origin: Origin::Child {
            program: "eml".into(),
            pipe: ChildPipe::Stdout,
        },
        rest: Vec::new(),
        start: 0,
    };
    let stream = unsafe { ctx.heap_mut() }.alloc_foreign(stream, true);
    let token = submit_read(
        &registry,
        &solo(&registry),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        buf,
        stream,
    )
    .unwrap();
    let op = CellRef::decode(token);
    assert_eq!(driver.submits, 1);
    assert_eq!(driver.lent.pop().unwrap().cap, READ_ROOM);
    let mut out = Vec::new();
    complete_op(
        &registry,
        &mut ctx,
        &mut driver,
        op,
        Err(io::Error::other("gone")),
        &mut out,
    );
    let taken = take_op(&registry, &mut ctx, token, 0).unwrap();
    let (_, message) = taken.failure().unwrap();
    assert_eq!(message, "cannot read the output of `eml`: gone");
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// 標準エラーの pipe は1回だけ取り出せ、読みの誤りの文言は最後の段の標準エラーを名指す (docs/spec/stdlib.md の
/// 「`Std.Exec`」)。
#[test]
fn a_stderr_pipe_names_the_errors_of_the_last_stage() {
    let mut ctx = test_ctx();
    let mut driver = NoDriver::default();
    let registry = Shared::new(Registry::new(Vec::new(), 1));
    let (child, _, _) = fake_child(&mut ctx, &["head", "tail"], true);
    unsafe { &mut *ctx.heap().foreign_unique::<ExecChild>(child).unwrap() }.stderr =
        Some(Bind::Bound {
            host: 0,
            handle: 0,
            bin: Arc::default(),
        });
    let reader = exec_take(&mut ctx, child, ChildPipe::Stderr)
        .unwrap()
        .unwrap();
    assert_eq!(
        exec_take(&mut ctx, child, ChildPipe::Stderr).unwrap(),
        Err("cannot take the errors of `tail`: not available".to_string())
    );
    let buf = bytes_ab(&mut ctx, 0);
    let token = submit_read(
        &registry,
        &solo(&registry),
        &mut ctx,
        Some(&mut driver),
        0,
        0,
        buf,
        reader,
    )
    .unwrap();
    let mut out = Vec::new();
    complete_op(
        &registry,
        &mut ctx,
        &mut driver,
        CellRef::decode(token),
        Err(io::Error::other("gone")),
        &mut out,
    );
    let taken = take_op(&registry, &mut ctx, token, 0).unwrap();
    let (_, message) = taken.failure().unwrap();
    assert_eq!(message, "cannot read the errors of `tail`: gone");
    unsafe { ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
    unsafe { ctx.heap_mut().decref(child) }.unwrap();
    let leaks = ctx.leaks();
    assert!(leaks.is_empty(), "{leaks:?}");
}
