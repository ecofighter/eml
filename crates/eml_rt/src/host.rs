//! セッションの Host (docs/spec/object-model.md の「extern」)。出力の先とファイルの開き方を、実行の環境ごとに替え
//! られるようにする。ネイティブ、WASM、アプリへの組み込み、REPL は、渡す Host を替えるだけで済む。
//!
//! Verus は `dyn Host` も `io::Error` も扱えないので、Host の trait と実装は `verus!` の外に置く。`Ctx` は Host を不透明な
//! `HostRef` で持ち、何も約束しない関数を通して呼ぶ (信頼の一覧の T11)。

use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, IsTerminal, Read, Write};
use std::mem::ManuallyDrop;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use vstd::prelude::*;

use crate::control::{Control, Stop};
use crate::externs::IoReason;

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
    reader: Box<dyn BufRead + Send>,
    /// まだ使っていない長さの見込み。1回目の `read_all` が取り出すので、2回目は余分に確保しない。
    size: Option<u64>,
}

/// 読みの誤り。`Box<str>` は File のパスである。
pub(crate) enum ReadError {
    Io(Box<str>, IoReason),
    NotUtf8(Box<str>),
    Stopped(Stop),
}

/// 1回に読むバイト数。読む間に止める語と確保量を見る間隔になる。
const READ_CHUNK: u64 = 64 * 1024;

/// Host に `path` を開かせる。誤りなら、写したパスと理由を返す。
#[verifier::external_body]
pub(crate) fn host_open(host: &HostRef, path: &str) -> Result<File, (Box<str>, IoReason)> {
    // パスは1回だけ写し、開けても開けなくても、それを持たせる
    let path: Box<str> = path.into();
    match host.host.open(&path) {
        Ok(Opened { reader, size }) => Ok(File { path, reader, size }),
        Err(e) => Err((path, io_reason(&e))),
    }
}

/// `file` を終わりまで読み、UTF-8 の文字列にする。
///
/// # Safety
/// `file` は生きている `File` を指し、読む間ほかに参照がない。呼ぶ側が `Heap::foreign_unique` で確かめる (T3)。
#[verifier::external_body]
pub(crate) unsafe fn read_text(file: *mut File, control: &Control) -> Result<String, ReadError> {
    let file = unsafe { &mut *file };
    let mut bytes = Vec::new();
    if let Some(size) = file.size.take() {
        // 確保に失敗したら I/O の誤りにする (`read_to_end` と同じ)
        let reserved = usize::try_from(size).ok().is_some_and(|n| bytes.try_reserve(n).is_ok());
        if !reserved {
            return Err(ReadError::Io(file.path.clone(), IoReason::Other));
        }
    }
    loop {
        let bits = control.stop_bits();
        if bits != 0 {
            return Err(
                ReadError::Stopped(
                    if bits & Control::INTERRUPT != 0 {
                        Stop::Interrupted
                    } else {
                        Stop::MemoryLimit
                    },
                ),
            );
        }
        // `Vec` はヒープの外にあるので、ページの確保量に足して上限と比べる

        if control.bytes().saturating_add(bytes.capacity()) > control.limits().max_bytes {
            return Err(ReadError::Stopped(Stop::MemoryLimit));
        }
        // `read_to_end` が、`Interrupted` の読み直しと短い読みを受け持つ

        match (&mut file.reader).take(READ_CHUNK).read_to_end(&mut bytes) {
            Ok(0) => break,
            Ok(_) => {},
            Err(e) => return Err(ReadError::Io(file.path.clone(), io_reason(&e))),
        }
    }
    String::from_utf8(bytes).map_err(|_| ReadError::NotUtf8(file.path.clone()))
}

} // verus!
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
// 「S9b-2b (実行文脈)」)
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
}

/// 開いたファイル。
pub struct Opened {
    // S12 で行ごとに読む extern を足すときに、そのまま使えるよう `BufRead` で渡す
    pub reader: Box<dyn BufRead + Send>,
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
/// 落とすときは、`BufWriter` の `Drop` がまだ書いていない出力を書く (unwind のときも同じで、誤りは捨てる)。
pub struct StdHost {
    // ストリームごとにロックを1つ持ち、ロックの中で本文と `\n` を書く
    out: Mutex<BufWriter<Box<dyn Write + Send>>>,
    err: Mutex<BufWriter<Box<dyn Write + Send>>>,
    /// 標準出力を行ごとに書くか。
    line: bool,
    file_root: PathBuf,
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
        }
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
            reader: Box::new(BufReader::new(file)),
            size,
        })
    }
}

/// I/O の誤りの理由。`Interrupted` は `read_to_end` が読み直すので、ここに届かない。
fn io_reason(e: &io::Error) -> IoReason {
    match e.kind() {
        io::ErrorKind::NotFound => IoReason::NotFound,
        io::ErrorKind::PermissionDenied => IoReason::PermissionDenied,
        _ => IoReason::Other,
    }
}
