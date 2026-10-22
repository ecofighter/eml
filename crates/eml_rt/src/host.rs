//! セッションの Host (docs/spec/object-model.md の「extern」)。出力の先とファイルの開き方を、実行の環境ごとに替え
//! られるようにする。ネイティブ、WASM、アプリへの組み込み、REPL は、渡す Host を替えるだけで済む。
//!
//! Verus は `dyn Host` も `io::Error` も扱えないので、Host の trait と実装は `verus!` の外に置く。`Ctx` は Host を不透明な
//! `HostRef` で持ち、何も約束しない関数を通して呼ぶ (信頼の一覧の T11)。

use std::ffi::OsString;
use std::fs;
use std::io::{self, BufWriter, IsTerminal, Read, Write};
use std::mem::ManuallyDrop;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::sync::{Arc, Mutex, PoisonError};

use vstd::prelude::*;

use crate::control::{Control, Stop, stop_reason};
use crate::externs::{Fault, IndexFault, IoOp, IoReason, Utf8Source};

verus! {

/// 書き込む先のストリーム。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

/// `Ctx` が持つ Host。
#[verifier::external_body]
pub struct HostRef {
    // `Drop` で `release_host` に渡して手放す。理由は `release_host` の注釈にある
    host: ManuallyDrop<Arc<dyn Host>>,
}

impl Clone for HostRef {
    #[verifier::external_body]
    fn clone(&self) -> Self {
        HostRef { host: ManuallyDrop::new(Arc::clone(&self.host)) }
    }
}

/// Host に1行を書かせる。誤りは `io::Error` の文言で返す。
#[verifier::external_body]
pub(crate) fn host_write_line(host: &HostRef, stream: Stream, line: &str) -> Result<(), Box<str>> {
    host.host.write_line(stream, line).map_err(|e| e.to_string().into_boxed_str())
}

/// Host がまとめて持っている出力を書かせる。誤りは `io::Error` の文言で返す。
#[verifier::external_body]
pub(crate) fn host_flush(host: &HostRef) -> Result<(), Box<str>> {
    host.host.flush().map_err(|e| e.to_string().into_boxed_str())
}

/// `Fs.open` で開いたファイル。`Foreign` の物体の中身にする。閉じる処理は drop glue だけで、どのスレッドで落として
/// もよい (docs/future/multicore.md)。
#[verifier::external_body]
pub(crate) struct File {
    /// `open` に渡されたままのパス。誤りの文言に使う。
    path: Box<str>,
    reader: Reader,
    /// まだ使っていない長さの見込み。1回目の読みが取り出すので、その後の `read_all` は余分に確保しない。
    size: Option<u64>,
}

/// 1回に読むバイト数。読み手の buffer の大きさで、読む間に止める語と確保量を見る間隔になる。
const READ_CHUNK: u64 = 64 * 1024;

/// 読みが途中で終わった理由。`Output` は、補充の前に標準出力を書き出せなかったことである。`NotUtf8` は、読んだバイト列が
/// UTF-8 でなかったことで、文字列にする `read_text` だけが作る。
pub(crate) enum Failure {
    Io(IoReason),
    Stopped(Stop),
    Output(Box<str>),
    NotUtf8,
}

/// buffer を持つ読み手 (docs/spec/object-model.md の「読み手」)。`fill` が最大 `READ_CHUNK` バイトを1回読み、
/// `consume` が使った分を進める。
#[verifier::external_body]
pub(crate) struct Reader {
    inner: Box<dyn Read + Send>,
    /// 全部の読みだけをする読み手は buffer を使わないので、最初の補充で確保する
    buf: Box<[u8]>,
    pos: usize,
    filled: usize,
}

/// Host が持つ標準入力の読み手の置き場 (docs/spec/object-model.md の「読み手」)。読む者が取り、buffer に残ったバイト
/// ごと返すので、同じ Host の上の次の `Ctx` も、読んでいない入力から続けて読める。
#[verifier::external_body]
pub struct StdinSlot {
    reader: Mutex<Option<Reader>>,
}

/// `src` の読み手の buffer に残ったバイトを返す。空なら1回補充してから返し、終わりなら空の slice を返す
/// (`Reader::fill_buf`)。`host` があれば、補充の前にその Host の出力を書き出させる (標準入力の読み)。誤りは `src` の
/// 名前を付けた `Fault` にする (`Source::fault`)。誤りの種類は `Source::fault` の実装で決まる。検証した `Ctx` の
/// コードは、`foreign_unique` で借りたハンドルのままこれを呼ぶ。
///
/// # Safety
/// `src` は生きている `S` を指し、呼ぶ間ほかに参照がない。返す slice を読めるのは、次に `src` を使うか (`fill`、
/// `consume`、そのほか `src` を受ける T11 の関数のどれでも)、`src` を落とすまでの間だけである。slice は読み手の
/// buffer を指し、ヒープのページを指さないので、その間にヒープで確保しても解放してもよい。
// 今は信頼の一覧のテストだけが呼ぶ。検証した `Ctx` のコードが読み手を直接読む入口として置く
#[allow(dead_code)]
#[verifier::external_body]
pub(crate) unsafe fn fill<'a, S: Source>(
    src: *mut S,
    control: &Control,
    host: Option<&HostRef>,
) -> Result<&'a [u8], Fault> {
    // `fill_buf` の slice の寿命は `src` の借用に縛られるが、ここで呼ぶ側の約束 (上の Safety) の寿命に付け替える
    let r: *mut Reader = unsafe { (*src).reader() };
    match unsafe { (*r).fill_buf(control, host) } {
        Ok(bytes) => Ok(bytes),
        Err(failure) => Err(unsafe { (*src).fault(failure) }),
    }
}

/// `fill` が返したバイトのうち、先頭の `n` バイトを使ったことにする (`Reader::consume`)。
///
/// # Safety
/// `src` は生きている `S` を指し、呼ぶ間ほかに参照がない。
#[allow(dead_code)]
#[verifier::external_body]
pub(crate) unsafe fn consume<S: Source>(src: *mut S, n: usize) {
    let reader = unsafe { (*src).reader() };
    reader.consume(n);
}

/// Host に `path` を開かせる。誤りなら、写したパスと理由を返す。
#[verifier::external_body]
pub(crate) fn host_open(host: &HostRef, path: &str) -> Result<File, (Box<str>, IoReason)> {
    // パスは1回だけ写し、開けても開けなくても、それを持たせる
    let path: Box<str> = path.into();
    match host.host.open(&path) {
        Ok(Opened { reader, size }) => Ok(File { path, reader: Reader::new(reader), size }),
        Err(e) => Err((path, io_reason(&e))),
    }
}

/// `src` を終わりまで (`line` なら次の `\n` まで、それを含めて) 読み、UTF-8 の文字列にする。`host` があれば、補充の
/// 前ごとにその Host の出力を書き出させる (`fill` と同じ)。誤りは `src` の名前を付けた `Fault` にする
/// (`Source::fault`)。`Fault::Heap` を返さないことは、`Source::fault` の実装がそれを作らないことに依る。
///
/// # Safety
/// `src` は生きている `S` を指し、読む間ほかに参照がない。呼ぶ側が `Heap::foreign_unique` で確かめる (T3)。
#[verifier::external_body]
pub(crate) unsafe fn read_text<S: Source>(
    src: *mut S,
    control: &Control,
    host: Option<&HostRef>,
    line: bool,
) -> (r: Result<String, Fault>)
    ensures
        !(r matches Err(Fault::Heap(_))),
{
    let src = unsafe { &mut *src };
    // 見込みはファイル全体の長さなので、1行の読みでは確保に使わない。読んだ後は残りの長さと合わないので捨てる
    let size = src.take_size().filter(|_| !line);
    let read = if line {
        read_line(src.reader(), control, host)
    } else {
        read_all(src.reader(), control, size, host)
    };
    let bytes = read.map_err(|f| src.fault(f))?;
    String::from_utf8(bytes).map_err(|_| src.fault(Failure::NotUtf8))
}

/// スクリプトに渡した引数の数。
#[verifier::external_body]
pub(crate) fn host_arg_count(host: &HostRef) -> usize {
    host.host.args().len()
}

/// `Env.args ()` の `index` 番の引数を写す。範囲の外と UTF-8 でない引数は誤りにする。
#[verifier::external_body]
pub(crate) fn host_arg(host: &HostRef, index: i64) -> (r: Result<String, Fault>)
    ensures
        !(r matches Err(Fault::Heap(_))),
{
    let args = host.host.args();
    let arg = usize::try_from(index).ok().and_then(|i| args.get(i)).ok_or_else(
        || { Fault::Index(Box::new(IndexFault::Arg { index, count: args.len() as i64 })) },
    )?;
    arg.to_str().map(str::to_string).ok_or_else(|| Fault::NotUtf8(Box::new(Utf8Source::Arg(index))))
}

/// 環境変数 `name` が設定されているか。
#[verifier::external_body]
pub(crate) fn host_env_has(host: &HostRef, name: &str) -> bool {
    host.host.env_var(name).is_some()
}

/// 環境変数 `name` の値を写す。設定されていなければ空の文字列で、UTF-8 でない値は誤りにする。
#[verifier::external_body]
pub(crate) fn host_env_var(host: &HostRef, name: &str) -> (r: Result<String, Fault>)
    ensures
        !(r matches Err(Fault::Heap(_))),
{
    match host.host.env_var(name) {
        None => Ok(String::new()),
        Some(value) => value.into_string().map_err(
            |_| Fault::NotUtf8(Box::new(Utf8Source::EnvVar(name.into()))),
        ),
    }
}

/// 標準入力の残りをすべて (`line` なら次の `\n` まで) 読む。読み手は Host の slot に置いたまま、slot のロックの下で
/// 読むので、誤りでも panic でも buffer に残ったバイトごと slot に残る。補充の前ごとに標準出力を書き出し、読みは
/// `read_text` と同じく止める語と確保量を見る。
#[verifier::external_body]
pub(crate) fn host_read_stdin(host: &HostRef, control: &Control, line: bool) -> (r: Result<
    String,
    Fault,
>)
    ensures
        !(r matches Err(Fault::Heap(_))),
{
    let not_available = || Fault::Stdin(IoReason::NotAvailable);
    let slot = host.host.stdin().ok_or_else(not_available)?;
    let read = slot.with_reader(
        |reader|
            {
                if line {
                    read_line(reader, control, Some(host))
                } else {
                    read_all(reader, control, None, Some(host))
                }
            },
    ).ok_or_else(not_available)?;
    match read {
        Ok(bytes) => String::from_utf8(bytes).map_err(
            |_| Fault::NotUtf8(Box::new(Utf8Source::Stdin)),
        ),
        Err(Failure::Io(reason)) => Err(Fault::Stdin(reason)),
        Err(Failure::Stopped(stop)) => Err(Fault::Stopped(stop)),
        Err(Failure::Output(message)) => Err(Fault::Output(message)),
        Err(Failure::NotUtf8) => Err(Fault::NotUtf8(Box::new(Utf8Source::Stdin))),
    }
}

/// Host に `path` のファイルを書かせる。
#[verifier::external_body]
pub(crate) fn host_write_file(host: &HostRef, path: &str, text: &str) -> (r: Result<(), Fault>)
    ensures
        !(r matches Err(Fault::Heap(_))),
{
    host.host.write_file(path, text).map_err(
        |e| Fault::Io { op: IoOp::Write, name: path.into(), reason: io_reason(&e) },
    )
}

/// Host に子のプロセスを起動させ、終了コードを返す。`args` は、引数の数と各引数を、それぞれ NUL で終えて並べた列
/// である。OS の引数は NUL を含めないので、NUL を含む引数があれば数が合わなくなり、起動の誤りにする。止める語が
/// 起動の前に立っていれば起動せず、子を待つ間に立てば子が終わった後で、止めたことにする
/// (docs/spec/object-model.md の「実行の制御」)。
#[verifier::external_body]
pub(crate) fn host_run(host: &HostRef, control: &Control, program: &str, args: &str) -> (r: Result<
    i64,
    Fault,
>)
    ensures
        !(r matches Err(Fault::Heap(_))),
{
    let stopped = ||
        {
            let bits = control.stop_bits();
            if bits != 0 {
                Err(Fault::Stopped(stop_reason(bits)))
            } else {
                Ok(())
            }
        };
    stopped()?;
    host.host.flush().map_err(|e| Fault::Output(e.to_string().into_boxed_str()))?;
    let run = ||
        {
            let mut fields = args.split_terminator('\0');
            let count = fields.next().and_then(|n| n.parse::<usize>().ok());
            let args: Vec<&str> = fields.collect();
            if count != Some(args.len()) {
                return Err(io::Error::from(io::ErrorKind::InvalidInput));
            }
            host.host.run(program, &args)
        };
    let code = run().map_err(
        |e| Fault::Io { op: IoOp::Run, name: program.into(), reason: io_reason(&e) },
    )?;
    stopped()?;
    Ok(code as i64)
}

} // verus!

/// 止める語が立った後の `Interrupted` を、`read_to_end` が読み直さない誤りに変える読み手 (docs/spec/object-model.md の
/// 「読み手」)。SIGINT のハンドラは `SA_RESTART` を外して入れるので、待っている読みは `EINTR` で戻る。止める語が
/// 立っていなければ `Interrupted` のまま返し、`read_to_end` に読み直させる。`read_buf` は stable では実装できないので、
/// `read_to_end` は空き領域を 0 で埋めてから読む。
struct StopOnInterrupt<'a> {
    reader: &'a mut (dyn Read + Send),
    control: &'a Control,
}

/// 止める語が立っていれば止め、`Vec` の持つ領域の大きさ `held` をページの確保量に足して上限と比べる。`Vec` はヒープの
/// 外にあり、そのままでは上限に数えられないためである。
fn check(control: &Control, held: usize) -> Result<(), Failure> {
    let bits = control.stop_bits();
    if bits != 0 {
        Err(Failure::Stopped(stop_reason(bits)))
    } else if control.over_budget(held) {
        Err(Failure::Stopped(Stop::MemoryLimit))
    } else {
        Ok(())
    }
}

/// 次の `\n` まで (それを含めて) 読む。終わりなら空である。std の `read_until` は `Interrupted` を自分で読み直すので、
/// 止める語を見られない。
fn read_line(
    reader: &mut Reader,
    control: &Control,
    host: Option<&HostRef>,
) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    loop {
        check(control, bytes.capacity())?;
        let buf = reader.fill_buf(control, host)?;
        if buf.is_empty() {
            return Ok(bytes);
        }
        let (used, done) = line_part(buf);
        bytes.extend_from_slice(&buf[..used]);
        reader.consume(used);
        if done {
            return Ok(bytes);
        }
    }
}

/// 1行の読みで `buf` から写すバイト数と、そこで行が終わるか。`\n` があればそれまで (それを含めて)、なければ全部である。
fn line_part(buf: &[u8]) -> (usize, bool) {
    match buf.iter().position(|&b| b == b'\n') {
        Some(i) => (i + 1, true),
        None => (buf.len(), false),
    }
}

/// 終わりまで読む。buffer に残ったバイトを先に取り、後は中の読み手を `READ_CHUNK` バイトずつ読みながら、止める語と
/// 確保量を見る。`size` は読む前に分かる長さで、それが上限を超える見込みなら、確保を試す前に止める。`host` があれば、
/// 中の読み手を読む前に1回、その Host の出力を書き出させる。
fn read_all(
    reader: &mut Reader,
    control: &Control,
    size: Option<u64>,
    host: Option<&HostRef>,
) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    if let Some(size) = size {
        // 確保できないほど大きい見込みも、上限があれば上限の誤りになる
        let size = usize::try_from(size).unwrap_or(usize::MAX);
        check(control, size)?;
        // 確保に失敗したら I/O の誤りにする (`read_to_end` と同じ)
        if bytes.try_reserve(size).is_err() {
            return Err(Failure::Io(IoReason::Other));
        }
    }
    bytes.extend_from_slice(reader.take_buffered());
    if let Some(host) = host {
        host.host
            .flush()
            .map_err(|e| Failure::Output(e.to_string().into_boxed_str()))?;
    }
    // 止める語が立っていれば、読みの誤りより止めたことを報告する。`read` が返した止めたことの誤りもここに来る
    let failed = |e: &io::Error| {
        let bits = control.stop_bits();
        if bits != 0 {
            Failure::Stopped(stop_reason(bits))
        } else {
            Failure::Io(io_reason(e))
        }
    };
    let mut inner = StopOnInterrupt {
        reader: &mut *reader.inner,
        control,
    };
    loop {
        check(control, bytes.capacity())?;
        // `read_to_end` が、止める語の立っていない `Interrupted` の読み直し、短い読み、終わりを小さな読みで確かめる
        // ことを受け持つ
        match (&mut inner).take(READ_CHUNK).read_to_end(&mut bytes) {
            Ok(0) => return Ok(bytes),
            Ok(_) => continue,
            Err(e) => return Err(failed(&e)),
        }
    }
}

impl Read for StopOnInterrupt<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.reader.read(buf) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted && self.control.stop_bits() != 0 => {
                Err(io::Error::other("stopped while reading"))
            }
            result => result,
        }
    }
}

impl Reader {
    pub(crate) fn new(inner: Box<dyn Read + Send>) -> Reader {
        Reader {
            inner,
            buf: Box::default(),
            pos: 0,
            filled: 0,
        }
    }

    /// buffer に残ったバイトを返す。空なら、止める語と確保量を見てから1回だけ読んで補充する。`host` があれば、補充の前に
    /// その Host の出力を書き出させる。終わりなら空の slice を返す。slice の寿命は `self` の借用に縛るので、次の
    /// `fill_buf` か `consume` の後に古い slice を読むコードはコンパイルできない。
    pub(crate) fn fill_buf(
        &mut self,
        control: &Control,
        host: Option<&HostRef>,
    ) -> Result<&[u8], Failure> {
        if self.pos < self.filled {
            return Ok(&self.buf[self.pos..self.filled]);
        }
        if self.buf.is_empty() {
            self.buf = vec![0; READ_CHUNK as usize].into_boxed_slice();
        }
        if let Some(host) = host {
            host.host
                .flush()
                .map_err(|e| Failure::Output(e.to_string().into_boxed_str()))?;
        }
        loop {
            check(control, 0)?;
            match self.inner.read(&mut self.buf) {
                // 渡した buffer より大きい長さを返す読み手は `Read` の約束を破っている。`pos` と `filled` を書く前に
                // 断るので、読み手は壊れずに次の読みへ残る
                Ok(n) if n > self.buf.len() => return Err(Failure::Io(IoReason::Other)),
                Ok(n) => {
                    self.pos = 0;
                    self.filled = n;
                    return Ok(&self.buf[..n]);
                }
                // 止める語が立っていない `Interrupted` は読み直す。立っていれば、次の周回の確かめが止める
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    let bits = control.stop_bits();
                    return Err(if bits != 0 {
                        Failure::Stopped(stop_reason(bits))
                    } else {
                        Failure::Io(io_reason(&e))
                    });
                }
            }
        }
    }

    /// `fill_buf` が返したバイトのうち、先頭の `n` バイトを使ったことにする。
    pub(crate) fn consume(&mut self, n: usize) {
        self.pos = self.filled.min(self.pos.saturating_add(n));
    }

    /// buffer に残ったバイトをすべて取り出す。全部の読みが、中の読み手を読む前に使う。
    fn take_buffered(&mut self) -> &[u8] {
        let rest = &self.buf[self.pos..self.filled];
        self.pos = self.filled;
        rest
    }
}

impl StdinSlot {
    /// `reader` を標準入力の読み手として置く。
    pub fn new(reader: Box<dyn Read + Send>) -> StdinSlot {
        StdinSlot {
            reader: Mutex::new(Some(Reader::new(reader))),
        }
    }

    /// slot のロックの下で、読み手を `f` に貸す。slot が空なら `None` である。読み手は slot に残るので、`f` が誤りを
    /// 返しても panic しても、buffer に残ったバイトは次の読みに残る。中身は読み手だけなので、ロックが毒されても続けて
    /// 使う。
    pub(crate) fn with_reader<R>(&self, f: impl FnOnce(&mut Reader) -> R) -> Option<R> {
        let mut reader = self.reader.lock().unwrap_or_else(PoisonError::into_inner);
        reader.as_mut().map(f)
    }
}

/// `fill` と `consume` が読む読み手のハンドル。`Foreign` の中身の型ごとに実装する。検証した `Ctx` のコードはハンドルの
/// 中身を開けないので、読み手を取り出すことと、誤りに名前を付けることを、信頼の一覧の `fill` の中でこの trait に
/// 任せる (docs/spec/object-model.md の「読み手」)。
pub(crate) trait Source {
    fn reader(&mut self) -> &mut Reader;
    /// まだ使っていない長さの見込みを取り出す。全部の読みが確保に使い、見込みのないハンドルは `None` を返す。
    fn take_size(&mut self) -> Option<u64>;
    /// 読みの誤りを、このハンドルの名前を付けた `Fault` にする。`Fault::Heap` は作らない (`read_text` の約束が
    /// これに依る)。
    fn fault(&self, failure: Failure) -> Fault;
}

impl Source for File {
    fn reader(&mut self) -> &mut Reader {
        &mut self.reader
    }

    fn take_size(&mut self) -> Option<u64> {
        self.size.take()
    }

    fn fault(&self, failure: Failure) -> Fault {
        match failure {
            Failure::Io(reason) => Fault::Io {
                op: IoOp::Read,
                name: self.path.clone(),
                reason,
            },
            Failure::Stopped(stop) => Fault::Stopped(stop),
            Failure::Output(message) => Fault::Output(message),
            Failure::NotUtf8 => Fault::NotUtf8(Box::new(Utf8Source::File(self.path.clone()))),
        }
    }
}

impl HostRef {
    pub fn new(host: Arc<dyn Host>) -> HostRef {
        HostRef {
            host: ManuallyDrop::new(host),
        }
    }
}

impl Drop for HostRef {
    fn drop(&mut self) {
        // `host` の欄はこの後で読まない
        release_host(unsafe { ManuallyDrop::take(&mut self.host) });
    }
}

/// Host の `Arc` を手放す。
// 欄の後始末として `Arc<dyn Host>` を手放す処理が展開されると、`Ctx` を使うコードのコード生成単位の分け方が変わりうる。
// その予防として展開させない (`Pages` の `control` の欄と同じ。docs/implementation/benchmarks.md の
// 「`eml_rt` のインライン化」)
#[inline(never)]
fn release_host(host: Arc<dyn Host>) {
    let _ = host;
}

/// 実行の環境。`par` の worker が同じ Host を共有するので、`Send + Sync` を求める。
pub trait Host: Send + Sync {
    /// 本文と `\n` を、ほかの行と途中で混ざらないように書く。
    fn write_line(&self, stream: Stream, line: &str) -> io::Result<()>;
    /// まとめて持っている出力を書く。
    fn flush(&self) -> io::Result<()>;
    /// `path` のファイルを読むために開く。
    fn open(&self, path: &str) -> io::Result<Opened>;
    // 次のメソッドの既定は、その入出力を持たない Host (組み込み先や REPL) の振る舞いである。使えない入出力は
    // `Unsupported` の誤りを返し、extern の誤りの理由は `not available` になる

    /// スクリプトに渡した引数 (`Env.args`)。`eml` とスクリプトのパスは含めない。
    fn args(&self) -> &[OsString] {
        &[]
    }
    /// 環境変数 `name` の値。設定されていなければ `None` である。
    fn env_var(&self, _name: &str) -> Option<OsString> {
        None
    }
    /// 標準入力の読み手の slot。`None` なら標準入力を使えない。
    fn stdin(&self) -> Option<Arc<StdinSlot>> {
        None
    }
    /// `path` のファイルを作るか切り詰めて、`text` を書く。
    fn write_file(&self, _path: &str, _text: &str) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
    /// `PATH` から `program` を探して子のプロセスを起動し、終わるのを待って終了コードを返す (`exit_code`)。子と eml の
    /// 出力が順に並ぶよう、起動の前に標準出力を書き出す。
    fn run(&self, _program: &str, _args: &[&str]) -> io::Result<i32> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

/// 子のプロセスの終了コード。シグナルで終わった子は、シェルと同じく 128 + シグナルの番号にする。
pub fn exit_code(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    status.code().unwrap_or(1)
}

/// 開いたファイル。
pub struct Opened {
    // buffer は `eml_rt` の読み手が持つので、Host は buffer のない読み手を渡せばよい
    pub reader: Box<dyn Read + Send>,
    /// 読む前に分かる長さ。ふつうのファイルなら、メタデータの長さである。
    pub size: Option<u64>,
}

/// 標準出力の書き方。標準エラーは、どちらでも行ごとに書く。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Buffering {
    /// 行ごとに書く。
    Line,
    /// 64 KiB ずつまとめて書く。
    Block,
}

/// 書き込みのバッファの大きさ。
const BUFFER_BYTES: usize = 64 * 1024;

/// 標準出力と標準エラーに書き、ファイルを `file_root` の下から開く Host。出力の書き方は C の stdio と同じにする。
/// 落とすときは、`BufWriter` の `Drop` がまだ書いていない出力を書く (unwind のときも同じで、誤りは捨てる)。環境変数は
/// プロセスのものを読み、子のプロセスは標準入出力と環境を受け継ぐ。
pub struct StdHost {
    // ストリームごとにロックを1つ持ち、ロックの中で本文と `\n` を書く
    out: Mutex<BufWriter<Box<dyn Write + Send>>>,
    err: Mutex<BufWriter<Box<dyn Write + Send>>>,
    /// 標準出力を行ごとに書くか。
    line: bool,
    file_root: PathBuf,
    args: Vec<OsString>,
    /// `None` なら標準入力を使えない (REPL)。
    stdin: Option<Arc<StdinSlot>>,
}

impl StdHost {
    /// 標準出力と標準エラーに書く。標準出力が端末なら、対話ですぐ見えるよう行ごとに書き、端末でなければまとめて書く。
    pub fn stdio(file_root: PathBuf) -> StdHost {
        let mode = if io::stdout().is_terminal() {
            Buffering::Line
        } else {
            Buffering::Block
        };
        StdHost::new(
            Box::new(io::stdout()),
            Box::new(io::stderr()),
            file_root,
            mode,
        )
    }

    pub fn new(
        out: Box<dyn Write + Send>,
        err: Box<dyn Write + Send>,
        file_root: PathBuf,
        mode: Buffering,
    ) -> StdHost {
        StdHost {
            out: Mutex::new(BufWriter::with_capacity(BUFFER_BYTES, out)),
            err: Mutex::new(BufWriter::with_capacity(BUFFER_BYTES, err)),
            line: mode == Buffering::Line,
            file_root,
            args: Vec::new(),
            stdin: None,
        }
    }

    /// スクリプトに渡す引数。
    pub fn with_args(mut self, args: Vec<OsString>) -> StdHost {
        self.args = args;
        self
    }

    /// 標準入力の読み手。渡さなければ、標準入力を使えない。
    pub fn with_stdin(mut self, stdin: Box<dyn Read + Send>) -> StdHost {
        self.stdin = Some(Arc::new(StdinSlot::new(stdin)));
        self
    }
}

/// ストリームのロックを取る。中身は書き込みのバッファだけなので、書き込み先の panic でロックが毒されても、続けて使って
/// 壊れることはない。
fn lock(
    stream: &Mutex<BufWriter<Box<dyn Write + Send>>>,
) -> std::sync::MutexGuard<'_, BufWriter<Box<dyn Write + Send>>> {
    stream.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Host for StdHost {
    fn write_line(&self, stream: Stream, line: &str) -> io::Result<()> {
        let (target, by_line) = match stream {
            Stream::Out => (&self.out, self.line),
            Stream::Err => (&self.err, true),
        };
        let mut w = lock(target);
        w.write_all(line.as_bytes())?;
        w.write_all(b"\n")?;
        if by_line {
            w.flush()?;
        }
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        let out = lock(&self.out).flush();
        let err = lock(&self.err).flush();
        out.and(err)
    }

    fn open(&self, path: &str) -> io::Result<Opened> {
        // `path` が絶対パスなら、`join` の結果はそのパスになる
        let file = fs::File::open(self.file_root.join(path))?;
        // ディレクトリも開けるが、長さの見込みはふつうのファイルにだけ使う
        let size = file
            .metadata()
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len());
        Ok(Opened {
            reader: Box::new(file),
            size,
        })
    }

    fn args(&self) -> &[OsString] {
        &self.args
    }

    fn env_var(&self, name: &str) -> Option<OsString> {
        // `var_os` は、空の名前と `=` か NUL を含む名前で panic しうる。どれも設定できない名前である
        if name.is_empty() || name.contains(['=', '\0']) {
            return None;
        }
        std::env::var_os(name)
    }

    fn stdin(&self) -> Option<Arc<StdinSlot>> {
        self.stdin.clone()
    }

    fn write_file(&self, path: &str, text: &str) -> io::Result<()> {
        fs::write(self.file_root.join(path), text)
    }

    fn run(&self, program: &str, args: &[&str]) -> io::Result<i32> {
        lock(&self.out).flush()?;
        let mut command = Command::new(program);
        command.args(args);
        // 空の根はプロセスの今のディレクトリである。`current_dir("")` は開けない
        if !self.file_root.as_os_str().is_empty() {
            command.current_dir(&self.file_root);
        }
        Ok(exit_code(command.status()?))
    }
}

/// I/O の誤りの理由 (`read_text`、`write_file`、`run` の失敗を `Fault` の理由にする)。止める語が立った後の
/// `Interrupted` は `fill` と `read_all` が止めたことにし、立っていない `Interrupted` は読み直す。
/// 読み以外の `Interrupted` は止める語と関係なく、ここで `Other` になる。
fn io_reason(e: &io::Error) -> IoReason {
    match e.kind() {
        io::ErrorKind::NotFound => IoReason::NotFound,
        io::ErrorKind::PermissionDenied => IoReason::PermissionDenied,
        io::ErrorKind::Unsupported => IoReason::NotAvailable,
        _ => IoReason::Other,
    }
}
