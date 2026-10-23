//! セッションの Host (docs/spec/object-model.md の「extern」)。出力の先とファイルの開き方を、実行の環境ごとに替え
//! られるようにする。ネイティブ、WASM、アプリへの組み込み、REPL は、渡す Host を替えるだけで済む。
//!
//! Verus は `dyn Host` も `io::Error` も扱えないので、Host の trait と実装は `verus!` の外に置く。`Ctx` は Host を不透明な
//! `HostRef` で持ち、何も約束しない関数を通して呼ぶ (信頼の一覧の T11)。

use std::ffi::OsString;
use std::fs;
use std::io::{self, BufWriter, IsTerminal, Read, Write};
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::process::{self, Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use vstd::prelude::*;

use crate::control::{Control, Stop, stop_reason};
use crate::externs::{Fault, Form, IndexFault, IoOp, IoReason, StdStream, Utf8Source};
use crate::heap::find_byte;
#[cfg(verus_only)]
use vstd::utf8::valid_utf8;

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
    /// 標準のストリームのハンドルの開いた印。写した `HostRef` (`par` の worker の `Ctx`) と共有する。
    flags: Arc<OpenFlags>,
}

impl Clone for HostRef {
    #[verifier::external_body]
    fn clone(&self) -> Self {
        HostRef { host: ManuallyDrop::new(Arc::clone(&self.host)), flags: Arc::clone(&self.flags) }
    }
}

/// 標準のストリームごとの、生きているハンドルがあるかの印 (docs/spec/object-model.md の「Host」)。開くときに立て、
/// ハンドルの `Drop` が下ろす。印は atomic なので、ハンドルを落とすスレッドは開いたスレッドと違ってよい
/// (docs/future/multicore.md)。
#[verifier::external_body]
pub(crate) struct OpenFlags {
    stdin: AtomicBool,
    stdout: AtomicBool,
    stderr: AtomicBool,
}

/// `Stdout.open` で開いた標準出力のハンドル。`Foreign` の物体の中身にする。buffer を持たず、Host の標準出力の buffer
/// を使う権利だけを表す (docs/spec/stdlib.md の「`Stdout` と `Stderr`」)。`Drop` は開いた印を下ろすだけである。
pub(crate) struct StdoutHandle {
    flags: Arc<OpenFlags>,
}

/// `Stderr.open` で開いた標準エラーのハンドル。`StdoutHandle` と別の型にし、`Heap::foreign` の型の確かめが2つの
/// ハンドルの取り違えを捕まえるようにする。
pub(crate) struct StderrHandle {
    flags: Arc<OpenFlags>,
}

/// `open_stream` が開いた、標準出力か標準エラーのハンドル。2つのハンドルは `open_stream` でしか作れないので、印と
/// ハンドルの型の組は、その `ensures` が決める。
pub(crate) enum OutputHandle {
    Stdout(StdoutHandle),
    Stderr(StderrHandle),
}

/// Host に1行を書かせる。誤りは `io::Error` の文言で返す。
#[verifier::external_body]
pub(crate) fn host_write_line(host: &HostRef, stream: Stream, line: &str) -> Result<(), Box<str>> {
    host.host.write_line(stream, line).map_err(|e| e.to_string().into_boxed_str())
}

/// Host に `bytes` をそのまま書かせる。誤りは `io::Error` の文言で返す。
#[verifier::external_body]
pub(crate) fn host_write(host: &HostRef, stream: Stream, bytes: &[u8]) -> Result<(), Box<str>> {
    host.host.write(stream, bytes).map_err(|e| e.to_string().into_boxed_str())
}

/// Host がまとめて持っている出力を書かせる。誤りは `io::Error` の文言で返す。
#[verifier::external_body]
pub(crate) fn host_flush(host: &HostRef) -> Result<(), Box<str>> {
    host.host.flush().map_err(|e| e.to_string().into_boxed_str())
}

/// 書くために開くときの、すでにあるファイルの扱い。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// 作るか、切り詰めて開く (`FileOut.create`)。
    Truncate,
    /// 作るか、末尾に足す形で開く (`FileOut.append`)。
    Append,
}

/// `Ctx::fs_open` が `Open::Write` で開いたファイル。`Foreign` の物体の中身にする。書き込みは自分の buffer にためる。
/// 閉じずに手放したときは、`BufWriter` の drop glue が残りを書き、誤りを捨てる (docs/spec/object-model.md の
/// 「出力のバッファ」)。
#[verifier::external_body]
pub(crate) struct FileOut {
    /// 開くときに渡されたままのパス。誤りの文言に使う。
    path: Box<str>,
    /// 書き込みの誤りの後は `None` にする。書いていない buffer を捨て、それ以上書かないためである。
    writer: Option<BufWriter<Box<dyn Write + Send>>>,
}

/// Host に `path` を書くために開かせる。誤りなら、写したパスと理由を返す。
#[verifier::external_body]
pub(crate) fn host_open_write(host: &HostRef, path: &str, mode: WriteMode) -> Result<
    FileOut,
    (Box<str>, IoReason),
> {
    let path: Box<str> = path.into();
    match host.host.open_write(&path, mode) {
        Ok(writer) => Ok(
            FileOut { path, writer: Some(BufWriter::with_capacity(BUFFER_BYTES, writer)) },
        ),
        Err(e) => Err((path, io_reason(&e))),
    }
}

/// `bytes` を (`line` なら続けて `\n` も) `f` の buffer に書く。buffer があふれれば Host の writer に書き出す。
/// 誤りなら buffer を捨て、`f` はそれ以上書かない。
///
/// # Safety
/// `f` は生きている `FileOut` を指し、呼ぶ間ほかに参照がない。呼ぶ側が `Heap::foreign_unique` で確かめる (T3)。
#[verifier::external_body]
pub(crate) unsafe fn file_out_write(f: *mut FileOut, bytes: &[u8], line: bool) -> (r: Result<
    (),
    Fault,
>)
    ensures
        r is Ok || r matches Err(Fault::Io { op: IoOp::Write, .. }),
{
    let f = unsafe { &mut *f };
    let Some(writer) = &mut f.writer else {
        return Err(write_fault(f, IoReason::NotAvailable));
    };
    let written = writer.write_all(bytes).and_then(
        |()|
            {
                if line {
                    writer.write_all(b"\n")
                } else {
                    Ok(())
                }
            },
    );
    written.map_err(|e| write_fault(f, io_reason(&e)))
}

/// `f` の buffer を Host の writer に書き出す。誤りなら、`file_out_write` と同じく buffer を捨てる。
///
/// # Safety
/// `file_out_write` と同じである。
#[verifier::external_body]
pub(crate) unsafe fn file_out_flush(f: *mut FileOut) -> (r: Result<(), Fault>)
    ensures
        r is Ok || r matches Err(Fault::Io { op: IoOp::Write, .. }),
{
    let f = unsafe { &mut *f };
    let flushed = match &mut f.writer {
        Some(writer) => writer.flush().map_err(|e| io_reason(&e)),
        None => Err(IoReason::NotAvailable),
    };
    flushed.map_err(|reason| write_fault(f, reason))
}

/// `FileIn.open` で開いたファイル。`Foreign` の物体の中身にする。閉じる処理は drop glue だけで、どのスレッドで落として
/// もよい (docs/future/multicore.md)。
#[verifier::external_body]
pub(crate) struct FileIn {
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

/// `Stdin.open` で開いた標準入力のハンドル。`Foreign` の物体の中身にする。開くときに Host の slot から読み手を取り、
/// 落とすときに buffer に残ったバイトごと slot に返す (docs/spec/object-model.md の「読み手」)。`Drop` は lock を取る
/// だけで、失敗せず I/O をしないので、どのスレッドで落としてもよい (docs/future/multicore.md)。
#[verifier::external_body]
pub(crate) struct StdinHandle {
    /// 開いている間はいつも `Some` である。Host が標準入力を持たないときだけ `None` で、読みは `not available` で
    /// 失敗する。
    reader: Option<Reader>,
    slot: Option<Arc<StdinSlot>>,
    /// 標準入力の開いた印。Host が標準入力を持たなくても立てるので、どの Host でも生きているハンドルは高々1つである。
    flags: Arc<OpenFlags>,
}

/// `src` の読み手の buffer に残ったバイトを返す。空なら1回補充してから返し、終わりなら空の slice を返す
/// (`Reader::fill_buf`)。`host` があれば、補充の前にその Host の出力を書き出させる (標準入力の読み)。誤りは `src` の
/// 名前を付けた `Fault` にする (`fault`)。検証した `Ctx` のコードは、`foreign_unique` で借りたハンドルのままこれを
/// 呼ぶ。
///
/// # Safety
/// `src` は生きている `S` を指し、呼ぶ間ほかに参照がない。返す slice を読めるのは、次に `src` を使うか (`fill`、
/// `consume`、そのほか `src` を受ける T11 の関数のどれでも)、`src` を落とすまでの間だけである。slice は読み手の
/// buffer を指し、ヒープのページを指さないので、その間にヒープで確保しても解放してもよい。
#[verifier::external_body]
pub(crate) unsafe fn fill<'a, S: Source>(
    src: *mut S,
    control: &Control,
    host: Option<&HostRef>,
) -> (r: Result<&'a [u8], Fault>)
    ensures
        !(r matches Err(Fault::Heap(_))),
{
    let src = unsafe { &mut *src };
    let Some(reader) = src.reader() else {
        return Err(fault(src.name(), Failure::Io(IoReason::NotAvailable)));
    };
    // `fill_buf` の slice の寿命は `src` の借用に縛られるが、ここで呼ぶ側の約束 (上の Safety) の寿命に付け替える
    let reader: *mut Reader = reader;
    match unsafe { (*reader).fill_buf(control, host) } {
        Ok(bytes) => Ok(bytes),
        Err(failure) => Err(fault(src.name(), failure)),
    }
}

/// `fill` が返したバイトのうち、先頭の `n` バイトを使ったことにする (`Reader::consume`)。
///
/// # Safety
/// `src` は生きている `S` を指し、呼ぶ間ほかに参照がない。
#[verifier::external_body]
pub(crate) unsafe fn consume<S: Source>(src: *mut S, n: usize) {
    if let Some(reader) = unsafe { (*src).reader() } {
        reader.consume(n);
    }
}

/// Host に `path` を開かせる。誤りなら、写したパスと理由を返す。
#[verifier::external_body]
pub(crate) fn host_open(host: &HostRef, path: &str) -> Result<FileIn, (Box<str>, IoReason)> {
    // パスは1回だけ写し、開けても開けなくても、それを持たせる
    let path: Box<str> = path.into();
    match host.host.open(&path) {
        Ok(Opened { reader, size }) => Ok(FileIn { path, reader: Reader::new(reader), size }),
        Err(e) => Err((path, io_reason(&e))),
    }
}

/// `src` を終わりまで (`line` なら次の `\n` まで、それを含めて) 読み、バイト列を返す。`form` が `Text` なら UTF-8 で
/// あることを確かめ、違えば誤りにする。`host` があれば、補充の前ごとにその Host の出力を書き出させる (`fill` と同じ)。
/// 誤りは `src` の名前を付けた `Fault` にする (`fault`)。
///
/// # Safety
/// `src` は生きている `S` を指し、読む間ほかに参照がない。呼ぶ側が `Heap::foreign_unique` で確かめる (T3)。
#[verifier::external_body]
pub(crate) unsafe fn read_text<S: Source>(
    src: *mut S,
    control: &Control,
    host: Option<&HostRef>,
    line: bool,
    form: Form,
) -> (r: Result<Vec<u8>, Fault>)
    ensures
        !(r matches Err(Fault::Heap(_))),
        form == Form::Text ==> (r matches Ok(v) ==> valid_utf8(v@)),
{
    let src = unsafe { &mut *src };
    // 見込みはファイル全体の長さなので、1行の読みでは確保に使わない。読んだ後は残りの長さと合わないので捨てる
    let size = src.take_size().filter(|_| !line);
    let read = match src.reader() {
        None => Err(Failure::Io(IoReason::NotAvailable)),
        Some(reader) if line => read_line(reader, control, host),
        Some(reader) => read_all(reader, control, size, host),
    };
    let bytes = read.map_err(|f| fault(src.name(), f))?;
    if form == Form::Text && str::from_utf8(&bytes).is_err() {
        return Err(fault(src.name(), Failure::NotUtf8));
    }
    Ok(bytes)
}

/// `Stdin.open`。開いた印を立て、Host の slot から読み手を取って、標準入力のハンドルを作る。印がすでに立っているか
/// slot が空なら、ほかのハンドルが開いているので `InUse` を返す。slot が空になるのは、同じ Host の別の `HostRef` の
/// ハンドルが読み手を持っているときである。Host が標準入力を持たなければ、読み手のないハンドルを作る
/// (docs/spec/stdlib.md の「ハンドル」)。
#[verifier::external_body]
pub(crate) fn open_stdin(host: &HostRef) -> (r: Result<StdinHandle, Fault>)
    ensures
        r is Ok || r matches Err(Fault::InUse(StdStream::Stdin)),
{
    let flags = host.flags.raise(StdStream::Stdin)?;
    let Some(slot) = host.host.stdin() else {
        return Ok(StdinHandle { reader: None, slot: None, flags });
    };
    match slot.take() {
        Some(reader) => Ok(StdinHandle { reader: Some(reader), slot: Some(slot), flags }),
        None => {
            flags.lower(StdStream::Stdin);
            Err(Fault::InUse(StdStream::Stdin))
        },
    }
}

/// `Stdout.open` と `Stderr.open`。`stream` の開いた印を立て、その印を持つ `stream` の型のハンドルを返す。印がすでに
/// 立っていれば、ほかのハンドルが開いているので `InUse` を返す (docs/spec/stdlib.md の「ハンドル」)。
#[verifier::external_body]
pub(crate) fn open_stream(host: &HostRef, stream: Stream) -> (r: Result<OutputHandle, Fault>)
    ensures
        r matches Ok(OutputHandle::Stdout(_)) ==> stream == Stream::Out,
        r matches Ok(OutputHandle::Stderr(_)) ==> stream == Stream::Err,
        r is Ok || (r matches Err(Fault::InUse(s)) && s == (if stream == Stream::Out {
            StdStream::Stdout
        } else {
            StdStream::Stderr
        })),
{
    match stream {
        Stream::Out => Ok(
            OutputHandle::Stdout(StdoutHandle { flags: host.flags.raise(StdStream::Stdout)? }),
        ),
        Stream::Err => Ok(
            OutputHandle::Stderr(StderrHandle { flags: host.flags.raise(StdStream::Stderr)? }),
        ),
    }
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

/// `Proc.spawn` で起動した子。`Foreign` の物体の中身にする。子のプロセスと、その標準入力への書き口と、標準出力の
/// 読み手を持つ。破棄処理は pipe を閉じてから子を kill して待つ (docs/spec/object-model.md の「子のプロセス」)。
#[verifier::external_body]
pub(crate) struct Child {
    /// 起動したプログラムの名前。誤りの文言に使う。
    program: Box<str>,
    /// `child_wait` が取り出した後は `None` で、破棄処理は待たない。
    process: Option<Box<dyn Process>>,
    /// `close_stdin` の後、書き込みの誤りの後、待つ前は `None` である。
    stdin: Option<Box<dyn Write + Send>>,
    stdout: Option<Reader>,
}

/// Host に子のプロセスを起動させる。`args` と `env` は、各要素を「バイト長の十進、NUL、本文」の順に並べた列で、
/// `env` はキーと値を交互に置く。`cwd` が空なら作業ディレクトリを設定しない。向きの値は、`stdin` と `stdout` が 0
/// (受け継ぐ)、1 (空)、2 (pipe)、`stderr` が 0 (受け継ぐ)、1 (標準出力に合わせる)、2 (捨てる) である。読めない列、
/// NUL を含む要素、範囲の外の向きは、起動の誤り (`InvalidInput`) にする。止める語が起動の前に立っていれば起動しない
/// (docs/spec/object-model.md の「実行の制御」)。
#[verifier::external_body]
#[expect(clippy::too_many_arguments, reason = "the seven arguments of `spawn_raw`")]
pub(crate) fn host_spawn(
    host: &HostRef,
    control: &Control,
    program: &str,
    args: &str,
    env: &str,
    cwd: &str,
    stdin: i64,
    stdout: i64,
    stderr: i64,
) -> (r: Result<Child, Fault>)
    ensures
        !(r matches Err(Fault::Heap(_))),
{
    let bits = control.stop_bits();
    if bits != 0 {
        return Err(Fault::Stopped(stop_reason(bits)));
    }
    host.host.flush().map_err(|e| Fault::Output(e.to_string().into_boxed_str()))?;
    let spawned = (||
        {
            let invalid = || io::Error::from(io::ErrorKind::InvalidInput);
            let stdio = |code: i64|
                match code {
                    0 => Some(Stdio::Inherit),
                    1 => Some(Stdio::Null),
                    2 => Some(Stdio::Pipe),
                    _ => None,
                };
            let args = fields(args).ok_or_else(invalid)?;
            let pairs = fields(env).ok_or_else(invalid)?;
            if pairs.len() % 2 != 0 || program.contains('\0') || cwd.contains('\0') {
                return Err(invalid());
            }
            let env: Vec<(&str, &str)> = pairs.chunks(2).map(|p| (p[0], p[1])).collect();
            let spawn = Spawn {
                program,
                args: &args,
                env: &env,
                cwd: if cwd.is_empty() {
                    None
                } else {
                    Some(cwd)
                },
                stdin: stdio(stdin).ok_or_else(invalid)?,
                stdout: stdio(stdout).ok_or_else(invalid)?,
                stderr: match stderr {
                    0 => StderrMode::Inherit,
                    1 => StderrMode::ToStdout,
                    2 => StderrMode::Null,
                    _ => return Err(invalid()),
                },
            };
            host.host.spawn(&spawn)
        })();
    match spawned {
        Ok(Spawned { stdin, stdout, process }) => Ok(
            Child {
                program: program.into(),
                process: Some(process),
                stdin,
                stdout: stdout.map(Reader::new),
            },
        ),
        Err(e) => Err(Fault::Io { op: IoOp::Run, name: program.into(), reason: io_reason(&e) }),
    }
}

/// `bytes` を (`line` なら続けて `\n` も) 子の標準入力に書き、書き出す。書き口がなければ (`close_stdin` の後) `not
/// available` の誤りにする。書き込みが `Interrupted` で戻るか失敗したとき、止める語が立っていれば、止めたことにする
/// (docs/spec/object-model.md の「実行の制御」)。誤りなら書き口を閉じる。
///
/// # Safety
/// `c` は生きている `Child` を指し、呼ぶ間ほかに参照がない。呼ぶ側が `Heap::foreign_unique` で確かめる (T3)。
#[verifier::external_body]
pub(crate) unsafe fn child_write(c: *mut Child, bytes: &[u8], line: bool, control: &Control) -> (r:
    Result<(), Fault>)
    ensures
        r is Ok || (r matches Err(Fault::Io { op: IoOp::Write, .. })) || (r matches Err(
            Fault::Stopped(_),
        )),
{
    let c = unsafe { &mut *c };
    let written = match &mut c.stdin {
        None => Err(Failure::Io(IoReason::NotAvailable)),
        Some(stdin) => write_stopping(&mut **stdin, bytes, line, control),
    };
    written.map_err(
        |failure|
            {
                c.stdin = None;
                match failure {
                    Failure::Stopped(stop) => Fault::Stopped(stop),
                    Failure::Io(reason) => Fault::Io {
                        op: IoOp::Write,
                        name: c.program.clone(),
                        reason,
                    },
                    // `write_stopping` は作らない
                    Failure::Output(_) | Failure::NotUtf8 => Fault::Io {
                        op: IoOp::Write,
                        name: c.program.clone(),
                        reason: IoReason::Other,
                    },
                }
            },
    )
}

/// 子の標準入力を閉じる。子に EOF が届く。
///
/// # Safety
/// `child_write` と同じである。
#[verifier::external_body]
pub(crate) unsafe fn child_close_stdin(c: *mut Child) {
    unsafe { (*c).stdin = None };
}

/// 子の標準入力を閉じ、子を取り出して待ち、終了コードを返す。待つ間に止める語が立てば、子が終わった後で、止めたことに
/// する (docs/spec/object-model.md の「実行の制御」)。
///
/// # Safety
/// `child_write` と同じである。
#[verifier::external_body]
pub(crate) unsafe fn child_wait(c: *mut Child, control: &Control) -> (r: Result<i64, Fault>)
    ensures
        !(r matches Err(Fault::Heap(_))),
{
    let c = unsafe { &mut *c };
    c.stdin = None;
    let waited = match c.process.take() {
        Some(mut process) => process.wait(),
        None => Err(io::Error::from(io::ErrorKind::Unsupported)),
    };
    let bits = control.stop_bits();
    if bits != 0 {
        return Err(Fault::Stopped(stop_reason(bits)));
    }
    match waited {
        Ok(code) => Ok(code as i64),
        Err(e) => Err(Fault::Io { op: IoOp::Run, name: c.program.clone(), reason: io_reason(&e) }),
    }
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
    match find_byte(buf, 0, b'\n') {
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
            // 渡した buffer より大きい長さを返す読み手は `Read` の約束を破っている。そのまま返すと `read_to_end` が
            // panic するので、`Reader::fill_buf` と同じく読みの誤りにする
            Ok(n) if n > buf.len() => Err(io::Error::other("read more than the buffer")),
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

    /// 読み手を取り出す。ハンドルが持っている間は空なので `None` を返す。
    pub(crate) fn take(&self) -> Option<Reader> {
        self.lock().take()
    }

    /// ハンドルが持っていた読み手を返す。
    fn put(&self, reader: Reader) {
        *self.lock() = Some(reader);
    }

    // 中身は読み手だけなので、ロックが毒されても続けて使う
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Reader>> {
        self.reader.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for StdinHandle {
    fn drop(&mut self) {
        if let (Some(slot), Some(reader)) = (&self.slot, self.reader.take()) {
            slot.put(reader);
        }
        // 読み手を返してから下ろすので、次に開いたハンドルは slot の読み手を取れる
        self.flags.lower(StdStream::Stdin);
    }
}

impl Drop for StdoutHandle {
    fn drop(&mut self) {
        self.flags.lower(StdStream::Stdout);
    }
}

impl Drop for StderrHandle {
    fn drop(&mut self) {
        self.flags.lower(StdStream::Stderr);
    }
}

impl OpenFlags {
    fn new() -> OpenFlags {
        OpenFlags {
            stdin: AtomicBool::new(false),
            stdout: AtomicBool::new(false),
            stderr: AtomicBool::new(false),
        }
    }

    fn flag(&self, stream: StdStream) -> &AtomicBool {
        match stream {
            StdStream::Stdin => &self.stdin,
            StdStream::Stdout => &self.stdout,
            StdStream::Stderr => &self.stderr,
        }
    }

    /// `stream` の印を立てる。すでに立っていれば `InUse` を返す。
    fn raise(self: &Arc<Self>, stream: StdStream) -> Result<Arc<OpenFlags>, Fault> {
        match self
            .flag(stream)
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(Arc::clone(self)),
            Err(_) => Err(Fault::InUse(stream)),
        }
    }

    fn lower(&self, stream: StdStream) {
        self.flag(stream).store(false, Ordering::Release);
    }
}

/// `fill` と `consume` が読む読み手のハンドル。`Foreign` の中身の型ごとに実装する。検証した `Ctx` のコードはハンドルの
/// 中身を開けないので、読み手を取り出すことと、誤りに付ける名前を、信頼の一覧の `fill` の中でこの trait に任せる
/// (docs/spec/object-model.md の「読み手」)。
pub(crate) trait Source {
    /// 読み手。`None` なら、このハンドルは読めず、読みは `not available` で失敗する。
    fn reader(&mut self) -> Option<&mut Reader>;
    /// まだ使っていない長さの見込みを取り出す。全部の読みが確保に使い、見込みのないハンドルは `None` を返す。
    fn take_size(&mut self) -> Option<u64>;
    /// 誤りの文言に使う名前。
    fn name(&self) -> SourceName;
}

/// 読みの誤りに付ける、読んだハンドルの名前。
pub(crate) enum SourceName {
    /// `open` に渡されたままのパス。
    FileIn(Box<str>),
    Stdin,
    /// 子の標準出力。プログラムの名前を持つ。
    Child(Box<str>),
}

/// 読みの誤りを、読んだハンドルの名前を付けた `Fault` にする。`Fault::Heap` を作らないので、`fill` と `read_text` の
/// 約束は `Source` の実装に依らない。
fn fault(name: SourceName, failure: Failure) -> Fault {
    match (failure, name) {
        (Failure::Io(reason), SourceName::FileIn(path)) => Fault::Io {
            op: IoOp::Read,
            name: path,
            reason,
        },
        (Failure::Io(reason), SourceName::Stdin) => Fault::Stdin(reason),
        (Failure::Stopped(stop), _) => Fault::Stopped(stop),
        (Failure::Output(message), _) => Fault::Output(message),
        (Failure::NotUtf8, SourceName::FileIn(path)) => {
            Fault::NotUtf8(Box::new(Utf8Source::FileIn(path)))
        }
        (Failure::NotUtf8, SourceName::Stdin) => Fault::NotUtf8(Box::new(Utf8Source::Stdin)),
        (Failure::Io(reason), SourceName::Child(program)) => Fault::Io {
            op: IoOp::ReadOutput,
            name: program,
            reason,
        },
        (Failure::NotUtf8, SourceName::Child(program)) => {
            Fault::NotUtf8(Box::new(Utf8Source::Child(program)))
        }
    }
}

/// 書き込みの誤り。`f` の書いていない buffer を、書き出さずに writer ごと捨てる。`BufWriter` の drop glue に任せると、
/// 誤りを返した後でもファイルが伸びうるためである (docs/spec/object-model.md の「誤りのときの約束」)。
fn write_fault(f: &mut FileOut, reason: IoReason) -> Fault {
    if let Some(writer) = f.writer.take() {
        let _ = writer.into_parts();
    }
    Fault::Io {
        op: IoOp::Write,
        name: f.path.clone(),
        reason,
    }
}

impl Source for FileIn {
    fn reader(&mut self) -> Option<&mut Reader> {
        Some(&mut self.reader)
    }

    fn take_size(&mut self) -> Option<u64> {
        self.size.take()
    }

    fn name(&self) -> SourceName {
        SourceName::FileIn(self.path.clone())
    }
}

impl Source for StdinHandle {
    fn reader(&mut self) -> Option<&mut Reader> {
        self.reader.as_mut()
    }

    fn take_size(&mut self) -> Option<u64> {
        None
    }

    fn name(&self) -> SourceName {
        SourceName::Stdin
    }
}

impl Source for Child {
    fn reader(&mut self) -> Option<&mut Reader> {
        self.stdout.as_mut()
    }

    fn take_size(&mut self) -> Option<u64> {
        None
    }

    fn name(&self) -> SourceName {
        SourceName::Child(self.program.clone())
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        // 先に pipe を閉じる。子が書き込みか読みで止まっていても、kill の前に EOF か EPIPE で戻れる。破棄処理は失敗
        // できず、どのスレッドで呼ばれてもよいので、誤りは捨てる (docs/spec/object-model.md の「子のプロセス」)
        self.stdin = None;
        self.stdout = None;
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
            let _ = process.wait();
        }
    }
}

impl HostRef {
    pub fn new(host: Arc<dyn Host>) -> HostRef {
        HostRef {
            host: ManuallyDrop::new(host),
            flags: Arc::new(OpenFlags::new()),
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
    /// `bytes` をそのまま、ほかの書き込みと途中で混ざらないように書く。
    fn write(&self, stream: Stream, bytes: &[u8]) -> io::Result<()>;
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
    /// `path` のファイルを書くために開く。`mode` が、すでにあるファイルを切り詰めるか、末尾に足すかを決める。書き手は
    /// buffer のない `Write` でよい。buffer は `eml_rt` の `FileOut` が持つ。
    fn open_write(&self, _path: &str, _mode: WriteMode) -> io::Result<Box<dyn Write + Send>> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
    /// `PATH` から `program` を探して子のプロセスを起動し、終わるのを待って終了コードを返す (`exit_code`)。子と eml の
    /// 出力が順に並ぶよう、起動の前に標準出力を書き出す。
    fn run(&self, _program: &str, _args: &[&str]) -> io::Result<i32> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
    /// 子のプロセスを起動し、pipe にした標準入力の書き口と標準出力の読み手を返す。子と eml の出力が順に並ぶよう、起動の
    /// 前に標準出力を書き出す。
    fn spawn(&self, _spawn: &Spawn<'_>) -> io::Result<Spawned> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

/// 子の標準入力か標準出力の向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stdio {
    /// eml のものを受け継ぐ。
    Inherit,
    /// 空にする (`/dev/null`)。
    Null,
    /// pipe にして、`Spawned` で eml に渡す。
    Pipe,
}

/// 子の標準エラーの向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StderrMode {
    /// eml のものを受け継ぐ。
    Inherit,
    /// 子の標準出力と同じ先に向ける (`2>&1`)。
    ToStdout,
    /// 捨てる (`2>/dev/null`)。
    Null,
}

/// `Host::spawn` に渡す、起動する子の設定。
#[derive(Debug)]
pub struct Spawn<'a> {
    /// `/` を含まなければ `PATH` から探す。`/` を含む相対パスは作業ディレクトリに対して解決する (`program_path`)。
    pub program: &'a str,
    pub args: &'a [&'a str],
    /// 上書きする環境変数。後の組が勝つ。
    pub env: &'a [(&'a str, &'a str)],
    /// 作業ディレクトリ。相対パスは Host の `file_root` に対して読む (`dir`)。
    pub cwd: Option<&'a str>,
    pub stdin: Stdio,
    pub stdout: Stdio,
    pub stderr: StderrMode,
}

impl Spawn<'_> {
    /// 子の作業ディレクトリ。`cwd` を `file_root` につなぐ。`cwd` がなければ `file_root` である。
    pub fn dir(&self, file_root: &Path) -> PathBuf {
        match self.cwd {
            Some(cwd) => file_root.join(cwd),
            None => file_root.to_path_buf(),
        }
    }

    /// 起動するプログラムのパス。`/` を含む相対パスは、作業ディレクトリ `dir` に対して絶対パスにする。Rust の
    /// `Command` は、`current_dir` と相対のプログラムの組み合わせの意味を決めていないためである。
    pub fn program_path(&self, dir: &Path) -> io::Result<PathBuf> {
        let program = Path::new(self.program);
        if self.program.contains('/') && program.is_relative() {
            std::path::absolute(dir.join(program))
        } else {
            Ok(program.to_path_buf())
        }
    }
}

/// 起動した子のプロセス。`std::process::Child` を Host の型に出さないのは、プロセスを持たない Host (組み込み先と
/// テスト) も実装できるようにするためである。
pub trait Process: Send {
    /// 子が終わるのを待ち、終了コード (`exit_code`) を返す。
    fn wait(&mut self) -> io::Result<i32>;
    fn kill(&mut self) -> io::Result<()>;
}

/// `Host::spawn` が起動した子。`Stdio::Pipe` にした向きだけが `Some` である。標準出力の読み手は buffer のない `Read`
/// でよい。buffer は `eml_rt` の読み手が持つ。
pub struct Spawned {
    pub stdin: Option<Box<dyn Write + Send>>,
    pub stdout: Option<Box<dyn Read + Send>>,
    pub process: Box<dyn Process>,
}

/// `std::process::Child` の `Process`。
struct OsProcess(process::Child);

impl Process for OsProcess {
    fn wait(&mut self) -> io::Result<i32> {
        Ok(exit_code(self.0.wait()?))
    }

    fn kill(&mut self) -> io::Result<()> {
        self.0.kill()
    }
}

/// `host_spawn` の列 (各要素を「バイト長の十進、NUL、本文」の順に並べたもの) を要素に分ける。読めない列と、NUL を
/// 含む要素は `None` にする。OS の引数と環境変数は NUL を含めないためである。
fn fields(mut list: &str) -> Option<Vec<&str>> {
    let mut out = Vec::new();
    while !list.is_empty() {
        let (len, rest) = list.split_once('\0')?;
        let len: usize = len.parse().ok()?;
        let field = rest.get(..len)?;
        if field.contains('\0') {
            return None;
        }
        out.push(field);
        list = &rest[len..];
    }
    Some(out)
}

/// `bytes` を (`line` なら続けて `\n` も) `w` にすべて書き、書き出す。std の `write_all` は `Interrupted` を自分で
/// 書き直すので、止める語を見られない。ここでは、止める語が立っていなければ書き直し、立っていれば止める。誤りのときも、
/// 止める語が立っていれば止めたことにする。Ctrl-C で子が先に終わり、書き込みが EPIPE で戻ったときも止めるためである。
fn write_stopping(
    w: &mut (dyn Write + Send),
    bytes: &[u8],
    line: bool,
    control: &Control,
) -> Result<(), Failure> {
    let failed = |e: &io::Error| {
        let bits = control.stop_bits();
        if bits != 0 {
            Failure::Stopped(stop_reason(bits))
        } else {
            Failure::Io(io_reason(e))
        }
    };
    let newline: &[u8] = if line { b"\n" } else { b"" };
    for mut part in [bytes, newline] {
        while !part.is_empty() {
            match w.write(part) {
                Ok(0) => return Err(failed(&io::Error::from(io::ErrorKind::WriteZero))),
                Ok(n) => part = &part[n.min(part.len())..],
                Err(e) if e.kind() == io::ErrorKind::Interrupted && control.stop_bits() == 0 => {}
                Err(e) => return Err(failed(&e)),
            }
        }
    }
    loop {
        match w.flush() {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted && control.stop_bits() == 0 => {}
            Err(e) => return Err(failed(&e)),
        }
    }
}

/// `Stdio` を std の向きにする。
fn os_stdio(stdio: Stdio) -> process::Stdio {
    match stdio {
        Stdio::Inherit => process::Stdio::inherit(),
        Stdio::Null => process::Stdio::null(),
        Stdio::Pipe => process::Stdio::piped(),
    }
}

/// 受け継いだ標準出力の fd の写し。`2>&1` で標準出力を受け継ぐ子の、標準エラーに渡す。
#[cfg(unix)]
fn inherited_stdout() -> io::Result<process::Stdio> {
    use std::os::fd::AsFd;
    Ok(process::Stdio::from(
        io::stdout().as_fd().try_clone_to_owned()?,
    ))
}

#[cfg(not(unix))]
fn inherited_stdout() -> io::Result<process::Stdio> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
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

/// 標準出力の書き方。標準エラーは、どちらでも書き込みごとに書き出す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Buffering {
    /// 行ごとに書く。`\n` を含む書き込みが書き出す。
    Line,
    /// 64 KiB ずつまとめて書く。
    Block,
}

/// 書き込みのバッファの大きさ。標準出力と標準エラーと `FileOut` が共有する。
pub(crate) const BUFFER_BYTES: usize = 64 * 1024;

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

    fn write(&self, stream: Stream, bytes: &[u8]) -> io::Result<()> {
        let (target, flush) = match stream {
            // 行ごとに書くときだけ改行を探す。まとめて書くときの書き込みに走査の費用を足さない
            Stream::Out => (&self.out, self.line && bytes.contains(&b'\n')),
            Stream::Err => (&self.err, true),
        };
        let mut w = lock(target);
        w.write_all(bytes)?;
        if flush {
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

    fn open_write(&self, path: &str, mode: WriteMode) -> io::Result<Box<dyn Write + Send>> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true);
        match mode {
            WriteMode::Truncate => options.truncate(true),
            WriteMode::Append => options.append(true),
        };
        Ok(Box::new(options.open(self.file_root.join(path))?))
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

    fn spawn(&self, spawn: &Spawn<'_>) -> io::Result<Spawned> {
        lock(&self.out).flush()?;
        let dir = spawn.dir(&self.file_root);
        let mut command = Command::new(spawn.program_path(&dir)?);
        command.args(spawn.args).envs(spawn.env.iter().copied());
        // 空の根はプロセスの今のディレクトリである。`current_dir("")` は開けない
        if !dir.as_os_str().is_empty() {
            command.current_dir(&dir);
        }
        command.stdin(os_stdio(spawn.stdin));
        let mut shared = None;
        match (spawn.stdout, spawn.stderr) {
            // 1本の pipe の書き口を2つに分けて、標準出力と標準エラーに渡す
            (Stdio::Pipe, StderrMode::ToStdout) => {
                let (reader, writer) = io::pipe()?;
                command.stderr(writer.try_clone()?).stdout(writer);
                shared = Some(reader);
            }
            (Stdio::Inherit, StderrMode::ToStdout) => {
                command
                    .stdout(process::Stdio::inherit())
                    .stderr(inherited_stdout()?);
            }
            (Stdio::Null, StderrMode::ToStdout) => {
                command
                    .stdout(process::Stdio::null())
                    .stderr(process::Stdio::null());
            }
            (stdout, StderrMode::Inherit) => {
                command
                    .stdout(os_stdio(stdout))
                    .stderr(process::Stdio::inherit());
            }
            (stdout, StderrMode::Null) => {
                command
                    .stdout(os_stdio(stdout))
                    .stderr(process::Stdio::null());
            }
        }
        let mut child = command.spawn()?;
        // `command` が持つ pipe の書き口を閉じる。残すと、子が終わっても読み手に終わりが届かない
        drop(command);
        let stdin = child
            .stdin
            .take()
            .map(|w| Box::new(w) as Box<dyn Write + Send>);
        let stdout = match shared {
            Some(reader) => Some(Box::new(reader) as Box<dyn Read + Send>),
            None => child
                .stdout
                .take()
                .map(|r| Box::new(r) as Box<dyn Read + Send>),
        };
        Ok(Spawned {
            stdin,
            stdout,
            process: Box::new(OsProcess(child)),
        })
    }
}

/// I/O の誤りの理由 (`read_text`、`open_write`、`FileOut` の書き込み、`run` の失敗を `Fault` の理由に
/// する)。止める語が立った後の `Interrupted` は `fill` と `read_all` が止めたことにし、立っていない `Interrupted` は
/// 読み直す。読み以外の `Interrupted` は止める語と関係なく、ここで `Other` になる。
fn io_reason(e: &io::Error) -> IoReason {
    match e.kind() {
        io::ErrorKind::NotFound => IoReason::NotFound,
        io::ErrorKind::PermissionDenied => IoReason::PermissionDenied,
        io::ErrorKind::Unsupported => IoReason::NotAvailable,
        _ => IoReason::Other,
    }
}
