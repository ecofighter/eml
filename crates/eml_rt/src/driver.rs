//! worker ごとの時計と I/O の待ち (docs/spec/vm.md の「時計と driver」)。実行系が持ち、`Ctx` には入れない。
//!
//! driver はヒープの語にも RC にも触れず、`RawBuf` を通してだけ記憶に届く。そのため `Host` の trait と同じく
//! `verus!` の外に置く。`external_body` を持たないので、信頼の一覧に行は要らない。

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

/// カーネルが読み書きする記憶。driver を持つスレッドの外へは出さないので `Send` ではない。`cap == 0` は
/// 記憶がないことを表し、`ptr` は空でない任意の番地で、読みも書きもしない。貸した側は、`submit` が
/// `Submit::Done` を返すか、その操作の完了を `completions` から取り出すまで、物体を生かし、動かさない。
/// `Read` に貸した記憶は一意に保ち、`Write` に貸した記憶は書き換えない。
#[derive(Clone, Copy)]
pub struct RawBuf {
    pub ptr: *mut u8,
    pub cap: usize,
}

/// driver に出す操作。
pub enum Op {
    /// `handle` の次の読みがブロックしない状態 (読めるデータがあるか EOF) なら `Ok(0)` で終わる。状態の問いで、
    /// 辺ではない。受け付けたときにその状態なら、その場で終わってよい。
    Readable { handle: IoHandle },
    /// `buf.ptr[0..cap]` に1回読む。読めるデータか EOF があればその場で読み、なければ読めるようになるまで
    /// 待ってから読む。`Ok(n)` で終わり、n == 0 は EOF である。`cap` は 1 以上。
    Read { handle: IoHandle, buf: RawBuf },
    /// `buf.ptr[0..cap]` をすべて書き、`Ok(cap)` で終わる。短く終わった書きは driver が続きを出し直す。
    Write { handle: IoHandle, buf: RawBuf },
    /// `addr` で TCP の接続を待ち受ける。`Done::Listener` で終わる。ポートが 0 なら空いているポートを使う。
    Listen { addr: SocketAddr },
    /// リスナー `listener` に来た接続を1つ受ける。`Done::Stream` で終わる。
    Accept { listener: IoHandle },
    /// `host` (IP アドレスの綴りか名前) の `port` へ TCP で接続する。名前は driver が解決し、得た番地を順に試して、
    /// 最後に試した番地の誤りを返す。`Done::Stream` で終わる。
    Connect { host: String, port: u16 },
    /// ブロックする呼び出しのプールの仕事。自分が所有する値 (所有して受け取る入力と、値で返す結果) を通してだけ
    /// 記憶に届き、ヒープにも RC にも触れない。panic しない。本物の driver は、仕事の panic を driver を持つ
    /// スレッドの上で起こし直すためである。
    Job(Box<dyn FnOnce() -> io::Result<usize> + Send + 'static>),
}

/// 終わった操作の中身。
#[derive(Debug, PartialEq, Eq)]
pub enum Done {
    /// `Readable`、`Read`、`Write`、`Job` の値。
    Size(usize),
    /// `Listen` が作ったリスナーと、待ち受けるポート。
    Listener { handle: IoHandle, port: u16 },
    /// `Accept` か `Connect` が作ったストリームと、相手の番地。
    Stream { handle: IoHandle, peer: SocketAddr },
}

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
/// driver を落とすときは、仕事も名前の解決も待たない。カーネルが buffer を持ちうる操作だけ終わりを待ち、上限までに
/// 終わらなければプロセスを止める。貸した記憶を放せないまま戻らないためである。
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
    /// 戻る。`Op::Job` には何もしない。
    fn cancel(&mut self, key: OpKey);
    /// `wait` (と `cancel` の即時の取り消し) が届けた完了をすべて `out` に移す。OS を見ず、保留の表を歩かない。
    /// 順は driver が決め、呼ぶ側は鍵で記録を引く。
    fn completions(&mut self, out: &mut Vec<Completion>);
    /// `Pending` を返して、まだ `completions` で返していない操作の数。
    fn pending(&self) -> usize;
    fn waker(&self) -> Arc<dyn IoWaker>;
    fn bin(&self) -> Arc<HandleBin>;
    /// ストリームの書く側を閉じる。相手は残りを読みきってから EOF を受ける。その場で終わる。
    fn shutdown_write(&mut self, handle: IoHandle) -> io::Result<()>;
    /// ストリームの `TCP_NODELAY` を切り替える。その場で終わる。
    fn set_nodelay(&mut self, handle: IoHandle, on: bool) -> io::Result<()>;
    /// driver の参照を落とす。保留の操作は終わるまで OS の資源を生かす。前提はない。
    fn close(&mut self, handle: IoHandle);
    /// 保留の操作をすべて取り消し、完了がすべて届くまで待って捨て、捨てた完了の中身のハンドルと `bin` のハンドルを
    /// 閉じる。仕事は上限なしに待ちきり、仕事の panic は待ちきった後で起こし直す。名前の解決は待たない。OS に出した
    /// 取り消しの終わりが上限までに届かなければ壊れた状態にする。driver が壊れているときは保留を残して戻る
    /// (呼ぶ側は `pending()` で確かめる)。
    fn quiesce(&mut self);
}
