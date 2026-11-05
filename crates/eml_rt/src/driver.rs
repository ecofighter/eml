//! worker ごとの時計と I/O の待ち (docs/spec/vm.md の「時計と driver」)。実行系が持ち、`Ctx` には入れない。
//!
//! driver はヒープの語にも RC にも触れず、`RawBuf` を通してだけ記憶に届く。そのため `Host` の trait と同じく
//! `verus!` の外に置く。`external_body` を持たないので、信頼の一覧に行は要らない。

use std::any::Any;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 呼ぶ側が振る操作の名前。1つの driver の中で、保留の操作 (`submit` が `Submit::Pending` を返し、まだ
/// `completions` で返していない操作) の鍵は互いに違う。`Submit::Done` を返した鍵と、完了を `completions` で
/// 返した鍵は、すぐに次の操作に使い回してよい。
pub type OpKey = u64;
/// driver が持つハンドル (リスナー、ストリーム)。1つの driver の寿命の間は使い回さない。
pub type IoHandle = u64;

/// カーネルとプールのスレッドが読み書きする記憶。貸す側のスレッドで作り、driver を持つスレッドと、その driver の
/// プールのスレッドで使うので `Send` である。番地を持つだけで、生かし続ける責めは持たない。その責めは下の貸す側の
/// 約束が負う。`cap == 0` は記憶がないことを表し、`ptr` は空でない任意の番地で、読みも書きもしない。貸した側は、
/// `submit` が `Submit::Done` を返すか、その操作の完了を `completions` から取り出すまで、物体を生かし、動かさない。
/// `Read` に貸した記憶は一意に保ち、`Write` に貸した記憶は書き換えない。
#[derive(Clone, Copy)]
pub struct RawBuf {
    pub ptr: *mut u8,
    pub cap: usize,
}

// SAFETY: `ptr` は、貸す側のスレッドで操作の記録が持つ物体から作る。記録は、`submit` が `Submit::Done` を返すか
// 完了を記録に当てるまで、その物体を生かし、動かさず、`Read` に貸した記憶は一意に保つ。その間 `RawBuf` を読み書きする
// のは、driver を持つスレッドと、その操作のファイルの仕事を走らせるプールのスレッドだけである。仕事はプールの列の
// mutex を通ってプールのスレッドへ渡り、答えは受け箱の mutex と `wait` と `completions` を通って戻るので、どちらの
// 受け渡しも happens-before の辺になり、貸す側が完了の後に読むか確定するのは仕事の書きの後になる
// (docs/spec/vm.md の「時計と driver」、docs/spec/object-model.md の T10 の行)。
unsafe impl Send for RawBuf {}

/// driver に出す操作。
pub enum Op {
    /// `handle` の次の読みがブロックしない状態 (読めるデータがあるか EOF) なら `Ok(0)` で終わる。状態の問いで、
    /// 辺ではない。受け付けたときにその状態なら、その場で終わってよい。
    Readable { handle: IoHandle },
    /// `buf.ptr[0..cap]` に1回読む。読めるデータか EOF があればその場で読み、なければ読めるようになるまで
    /// 待ってから読む。`Ok(n)` で終わり、n == 0 は EOF である。`cap` は 1 以上。ファイルのハンドルでは
    /// プールの仕事が読み、いつも `Submit::Pending` を返す。
    Read { handle: IoHandle, buf: RawBuf },
    /// `buf.ptr[0..cap]` をすべて書き、`Ok(cap)` で終わる。短く終わった書きは driver が続きを出し直す。
    /// ファイルのハンドルではプールの仕事が書き、いつも `Submit::Pending` を返す。
    Write { handle: IoHandle, buf: RawBuf },
    /// `addr` で TCP の接続を待ち受ける。`Done::Listener` で終わる。ポートが 0 なら空いているポートを使う。
    Listen { addr: SocketAddr },
    /// リスナー `listener` に来た接続を1つ受ける。`Done::Accepted` か `Done::Stream` で終わる。
    Accept { listener: IoHandle },
    /// `host` (IP アドレスの綴りか名前) の `port` へ TCP で接続する。名前は driver が解決し、得た番地を順に試して、
    /// 最後に試した番地の誤りを返す。`Done::Stream` で終わる。
    Connect { host: String, port: u16 },
    /// ストリーム `handle` の書く側を閉じる。相手は残りを読みきってから EOF を受ける。いつも受け付けで終わり、
    /// `Submit::Done(Ok(Done::Size(0)))` か `Submit::Done(Err(_))` を返す。
    ShutdownWrite { handle: IoHandle },
    /// ストリーム `handle` の `TCP_NODELAY` を切り替える。いつも受け付けで終わり、`Submit::Done(Ok(Done::Size(0)))` か
    /// `Submit::Done(Err(_))` を返す。
    SetNodelay { handle: IoHandle, on: bool },
    /// ブロックする呼び出しのプールの仕事。自分が所有する値 (所有して受け取る入力と、値で返す結果) を通してだけ
    /// 記憶に届き、ヒープにも RC にも触れない。panic しない。本物の driver は、仕事の panic を driver を持つ
    /// スレッドの上で起こし直すためである。所有した値を返し、`Done::Job` で終わる。失敗は値の中に書くので、
    /// 完了の `Err` は driver が仕事を走らせられなかったことだけを指す。
    Job(Box<dyn FnOnce() -> Box<dyn Any + Send> + Send + 'static>),
    /// どの driver にも縛っていない OS の資源を取り込み、`Done::Handle` で返す。いつも受け付けで終わり、受けた
    /// driver の `adopt` と同じ規則で `Submit::Done` を返す。
    Adopt { res: RawResource },
    /// 子のプロセス `process` が終わるまで待ち、`Done::Size(0)` で終わる。driver は子を回収しない。回収は Host の
    /// `Process::wait` が行う。すでに終わって回収していない子なら、その場か次の `wait` で終わる。
    Wait { process: RawProcess },
}

/// 終わった操作の中身。
pub enum Done {
    /// `Readable`、`Read`、`Write` の値。`ShutdownWrite`、`SetNodelay`、`Wait` は 0 である。
    Size(usize),
    /// `Listen` が作ったリスナーと、待ち受けるポート。
    Listener { handle: IoHandle, port: u16 },
    /// `Accept` か `Connect` が作ったストリームと、相手の番地。
    Stream { handle: IoHandle, peer: SocketAddr },
    /// `Accept` が受けて、まだどの driver にも結び付けていないソケットと、相手の番地。`adopt` した driver の
    /// ハンドルになる。
    Accepted { sock: RawSocket, peer: SocketAddr },
    /// `Adopt` が取り込んだ資源のハンドル。
    Handle(IoHandle),
    /// `Job` が返した値。受け取る側が型を知っていて、`downcast` で取り出す。
    Job(Box<dyn Any + Send>),
}

// 値の仕事の中身は `Debug` を持たないので、`Job` は中を書かない。
impl fmt::Debug for Done {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Done::Size(n) => f.debug_tuple("Size").field(n).finish(),
            Done::Listener { handle, port } => f
                .debug_struct("Listener")
                .field("handle", handle)
                .field("port", port)
                .finish(),
            Done::Stream { handle, peer } => f
                .debug_struct("Stream")
                .field("handle", handle)
                .field("peer", peer)
                .finish(),
            Done::Accepted { sock, peer } => f
                .debug_struct("Accepted")
                .field("sock", sock)
                .field("peer", peer)
                .finish(),
            Done::Handle(h) => f.debug_tuple("Handle").field(h).finish(),
            Done::Job(_) => f.write_str("Job(..)"),
        }
    }
}

/// OS のハンドルの持ち主の型。ファイルと子のパイプを driver と Host の間で受け渡す。
#[cfg(unix)]
pub type OsHandle = std::os::fd::OwnedFd;
#[cfg(windows)]
pub type OsHandle = std::os::windows::io::OwnedHandle;

/// Host が開いた、同期の通常のファイル。どの driver にも縛っていないので、どのスレッドへ移しても、そこで落としても
/// よい。落とせばファイルを閉じる。driver はその読み書きをプールの仕事にする (docs/spec/vm.md の「時計と driver」)。
#[derive(Debug)]
pub struct RawFile(OsHandle);

impl RawFile {
    pub fn new(handle: OsHandle) -> RawFile {
        RawFile(handle)
    }

    pub fn into_inner(self) -> OsHandle {
        self.0
    }
}

/// `IoDriver::pipe` が作った、親の側のパイプの端。どの driver にも縛っていないので、どのスレッドへ移しても、そこで
/// 落としてもよい。backend が使えるように用意してある (unix はブロックしない)。最初に操作を出す worker の driver が
/// `adopt` で取り込む (docs/spec/vm.md の「時計と driver」)。
#[derive(Debug)]
pub struct RawPipe(OsHandle);

impl RawPipe {
    pub fn new(handle: OsHandle) -> RawPipe {
        RawPipe(handle)
    }

    pub fn into_inner(self) -> OsHandle {
        self.0
    }
}

/// `Op::Wait` が待つ子のプロセス。unix はプロセスの番号で、持ち主は Host の子である。
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawProcess(pub i32);
/// Windows は Host が写したプロセスのハンドルである。
#[cfg(windows)]
#[derive(Debug)]
pub struct RawProcess(pub OsHandle);

/// `IoDriver::pipe` の結果。`child` は子に渡す端で、ブロックし、子のプロセスに受け継がれない (unix は
/// close-on-exec、Windows は継承しない設定)。起動が子のためにだけ受け継がせる。
#[derive(Debug)]
pub struct PipePair {
    pub parent: RawPipe,
    pub child: OsHandle,
}

/// `adopt` に渡す、どの driver にも縛っていない OS の資源。`PipeRead` は親が読む端、`PipeWrite` は親が書く端である。
#[derive(Debug)]
pub enum RawResource {
    Socket(RawSocket),
    PipeRead(RawPipe),
    PipeWrite(RawPipe),
    File(RawFile),
}

/// OS のソケットの持ち主の型。
#[cfg(unix)]
pub type OsSocket = std::os::fd::OwnedFd;
#[cfg(windows)]
pub type OsSocket = std::os::windows::io::OwnedSocket;

/// どの driver にも登録も結び付けもしていない OS のソケット。driver の外の値なので、どのスレッドへ移しても、そこで
/// 落としてもよい。落とせばソケットを閉じる。`adopt` した driver だけがそのハンドルとして使う
/// (docs/spec/vm.md の「時計と driver」)。
#[derive(Debug)]
pub struct RawSocket(OsSocket);

impl RawSocket {
    pub fn new(sock: OsSocket) -> RawSocket {
        RawSocket(sock)
    }

    pub fn into_inner(self) -> OsSocket {
        self.0
    }

    #[cfg(unix)]
    fn raw(&self) -> u64 {
        use std::os::fd::AsRawFd;
        self.0.as_raw_fd() as u64
    }

    #[cfg(windows)]
    fn raw(&self) -> u64 {
        use std::os::windows::io::AsRawSocket;
        self.0.as_raw_socket()
    }
}

// 開いている間は記述子の番号がソケットを決めるので、同じソケットを指すかで比べる
impl PartialEq for RawSocket {
    fn eq(&self, other: &RawSocket) -> bool {
        self.raw() == other.raw()
    }
}

impl Eq for RawSocket {}

/// `submit` の結果。
#[derive(Debug)]
pub enum Submit {
    /// その場で終わった。完了は `completions` に届かず、`pending` にも数えない。
    Done(io::Result<Done>),
    /// 後で `completions` にちょうど1回届く。
    Pending,
}

/// 終わった操作の結果。
pub struct Completion {
    pub key: OpKey,
    pub result: io::Result<Done>,
    /// `cancel` (か `quiesce`) のためだけに操作が終わった。そのとき `result` は `Err` である。取り消しを頼んだ後でも、
    /// 操作が自分で着いた結果なら偽である (docs/spec/vm.md の「時計と driver」)。
    pub cancelled: bool,
}

/// どのスレッドからも呼べる。シグナルの handler からは呼ばない。
pub trait IoWaker: Send + Sync {
    fn wake(&self);
}

/// `Lin` の `Foreign` が落とされたハンドル。driver は次の `wait`、`completions`、`quiesce` と drop のときに閉じる。
#[derive(Default)]
pub struct HandleBin {
    pub dropped: Mutex<Vec<IoHandle>>,
}

/// worker ごとの時計と I/O の待ち。driver は作ったスレッドの上でだけ使うので、`Send` も `Sync` も求めない。
/// スレッドをまたぐインターフェースは `IoWaker` だけである (docs/spec/vm.md の「時計と driver」)。
///
/// driver を落とすときは、仕事も名前の解決も待たない。OS かプールのスレッドがこちらの記憶 (貸した buffer か driver の
/// 記録) に書きうる操作 (ファイルの読み書きを含む) だけ終わりを待ち、上限までに終わらないか待ちが失敗すればプロセスを
/// 止める。その記憶を放せないまま戻らないためである。
pub trait IoDriver {
    /// 作ったときからの単調な経過時間。
    fn now(&self) -> Duration;
    /// `now() >= deadline_ms` か、完了が届くか、waker が鳴るまで待つ。OS の readiness と完了を待つのは `wait`
    /// だけで、`deadline_ms` が今以前でも OS を1回確かめ、終わった操作を完了として届けてから戻る。ほかの理由で早く
    /// 戻ってよいが、内部の刻みでは戻らない。`Ok(false)` を返すのは、`deadline_ms` が `None` で、期限なしには
    /// 完了が届きえないときだけである。保留の操作も届いた完了もないときがそれに当たり、仮想の時計の driver では、
    /// 保留の操作のどれも終わりえないときも当たる。`deadline_ms == u64::MAX` は期限なしで待つが、`Ok(false)` は返さず、
    /// waker が鳴れば `Ok(true)` で戻る。`Ok(false)` は外からの入力なしには何も進まないこと、`Err` は driver が
    /// 壊れたことを表す。
    fn wait(&mut self, deadline_ms: Option<u64>) -> io::Result<bool>;
    /// 失敗せず、ブロックしない。出せなかった操作は `Submit::Done(Err(_))` で返す。壊れていない driver では
    /// `Op::Job` はいつも `Pending` を返し、プールが満ちていても呼んだスレッドを止めない。壊れた driver は OS の資源も
    /// 仕事も作らず、`Op::Job` も含めて `Submit::Done(Err(_))` を返す。保留の鍵と同じ鍵で呼んではならない。
    fn submit(&mut self, key: OpKey, op: Op) -> Submit;
    /// 呼んだ時点で保留の鍵 `key` の操作だけを、できるだけ取り消す。保留でない鍵には何もしない。後で同じ鍵で
    /// 出した操作に効いてはならない (取り消しを後で OS に出す driver は、出す時点で同じ操作であることを確かめる)。
    /// 保留の操作の完了は、取り消しで終わったか自分で着いた結果で、それでも1回届く。ブロックしない。OS が取り消しを
    /// 断れば driver は壊れた状態になる。壊れた driver では完了が届かないことがあり、そのとき `quiesce` は保留を残して
    /// 戻る。`Op::Job` とファイルの読み書きには何もしない。
    fn cancel(&mut self, key: OpKey);
    /// `wait` (と `cancel` の即時の取り消し) が届けた完了をすべて `out` に移す。OS を見ず、保留の表を歩かない。
    /// 順は driver が決め、呼ぶ側は鍵で記録を引く。
    fn completions(&mut self, out: &mut Vec<Completion>);
    /// `Pending` を返して、まだ `completions` で返していない操作の数。
    fn pending(&self) -> usize;
    fn waker(&self) -> Arc<dyn IoWaker>;
    fn bin(&self) -> Arc<HandleBin>;
    /// driver の参照を落とす。保留の操作は終わるまで OS の資源を生かす。前提はない。
    fn close(&mut self, handle: IoHandle);
    /// 資源を自分のハンドルにして返す。`Done::Accepted` のソケットとパイプの端は自分に登録するか結び付ける。どの
    /// driver が受けたものでもよい。ファイルは登録も結び付けもせず、読み書きをプールの仕事にする。失敗すれば資源は
    /// 閉じる。ソケットを外に出さない driver はソケットに、パイプやファイルを扱わない driver はそれに `Err` を返し、
    /// 壊れた driver はどれにも `Err` を返す。
    fn adopt(&mut self, res: RawResource) -> io::Result<IoHandle>;
    /// 子と親をつなぐパイプを1組作る。`parent_reads` が真なら親が読み、子が書く。`SPAWN_LOCK` を持つ間に作って旗を
    /// 立てるので、同時に起動するほかの子は子の端を受け継がない。親の端はどの driver にも縛らずに返す。パイプを扱わない
    /// driver と壊れた driver は `Err` を返す (docs/spec/vm.md の「時計と driver」)。
    fn pipe(&mut self, parent_reads: bool) -> io::Result<PipePair>;
    /// 保留の操作をすべて取り消し、完了がすべて届くまで待って捨て、捨てた完了の中身のハンドルと `bin` のハンドルを
    /// 閉じる。仕事は上限なしに待ちきり、仕事の panic は待ちきった後で起こし直す。名前の解決は待たない。OS に出した
    /// 取り消しの終わりが上限までに届かなければ壊れた状態にする。driver が壊れているときは保留を残して戻る
    /// (呼ぶ側は `pending()` で確かめる)。
    fn quiesce(&mut self);
}
