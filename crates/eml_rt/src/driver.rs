//! worker ごとの時計と I/O の待ち (docs/spec/vm.md の「時計と driver」)。実行系が持ち、`Ctx` には入れない。
//!
//! driver はヒープの語にも RC にも触れず、`RawBuf` を通してだけ記憶に届く。そのため `Host` の trait と同じく
//! `verus!` の外に置く。`external_body` を持たないので、信頼の一覧に行は要らない。

use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// driver が振り、1つの driver の寿命の間は使い回さない。`submit` のたびに新しい値を返す。
pub type OpKey = u64;
/// driver が持つハンドル (今は `pair` の両端)。1つの driver の寿命の間は使い回さない。
pub type IoHandle = u64;

/// カーネルが読み書きする記憶。driver を持つスレッドの外へは出さないので `Send` ではない。`cap == 0` は
/// 記憶がないことを表し、`ptr` は空でない任意の番地で、読みも書きもしない。貸した側は、その操作の完了を
/// `completions` から取り出すまで、物体を生かし、一意に保ち、動かさない。
#[derive(Clone, Copy)]
pub struct RawBuf {
    pub ptr: *mut u8,
    pub cap: usize,
}

/// driver に出す操作。
pub enum Op {
    /// `handle` の読みがブロックしなくなったら (データか EOF) `Ok(0)` で終わる。
    Readable { handle: IoHandle },
    /// `buf.ptr[0..cap]` に読む。`Ok(n)` で終わり、n == 0 は EOF。
    Read { handle: IoHandle, buf: RawBuf },
    /// `buf.ptr[0..cap]` を書く。`Ok(n)` で終わり、n は短いことがある。
    Write { handle: IoHandle, buf: RawBuf },
    /// ブロックする呼び出しのプールの仕事。自分が所有する値 (所有して受け取る入力と、値で返す結果) を通してだけ
    /// 記憶に届き、ヒープにも RC にも触れない。panic しない。本物の driver は、仕事の panic を driver を持つ
    /// スレッドの上で起こし直すためである。
    Job(Box<dyn FnOnce() -> io::Result<usize> + Send + 'static>),
}

/// 終わった操作の結果。
pub struct Completion {
    pub key: OpKey,
    pub result: io::Result<usize>,
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
/// スレッドをまたぐ口は `IoWaker` だけである (docs/spec/vm.md の「時計と driver」)。
pub trait IoDriver {
    /// 作ったときからの単調な経過時間。
    fn now(&self) -> Duration;
    /// `now() >= deadline_ms` か、完了が届くか、waker が鳴るまで待つ。早く戻ってよい。
    /// `Ok(false)` は外からの入力なしには何も進まないこと、`Err` は driver が壊れたことを表す。
    fn wait(&mut self, deadline_ms: Option<u64>) -> io::Result<bool>;
    /// 失敗しない。出せなかった操作は、次の `completions` で誤りとして終わる。本物の driver では、プールの
    /// スレッドがすべて仕事中なら、仕事の `submit` はスレッドが空くまで、安全点を通らずに呼んだスレッドの上で待つ。
    fn submit(&mut self, op: Op) -> OpKey;
    /// できるだけ取り消す。`key` の完了はふつうはそれでも届く。届かないことがあれば、driver は `quiesce` で壊れた
    /// 状態になる。`Op::Job` には何もしない。
    fn cancel(&mut self, key: OpKey);
    /// 届いた完了をすべて `out` に移す。順は driver が決める。
    fn completions(&mut self, out: &mut Vec<Completion>);
    /// 出してまだ `completions` で返していない操作の数。
    fn pending(&self) -> usize;
    fn waker(&self) -> Arc<dyn IoWaker>;
    fn bin(&self) -> Arc<HandleBin>;
    /// つながった2つの端。
    fn pair(&mut self) -> io::Result<(IoHandle, IoHandle)>;
    /// driver の参照を落とす。保留の操作は、終わるまで OS の資源を生かす。前提はない。
    fn close(&mut self, handle: IoHandle);
    /// 保留の操作をすべて取り消し、完了がすべて届くまで待って捨て、`bin` のハンドルを閉じる。仕事は待ちきる。
    /// driver が壊れているか、取り消した操作の完了がいつまでも届かず壊れた状態になったときは、保留を残して戻る
    /// (呼ぶ側は `pending()` で確かめる)。
    fn quiesce(&mut self);
}
