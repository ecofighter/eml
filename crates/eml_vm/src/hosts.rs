//! VM を動かす Host (docs/spec/vm.md の「置き場所」)。`eml run` と REPL の Host と、テストが出力を読み、入力を与える
//! Host を置く。

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, Cursor, PipeReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use eml_rt::{
    Buffering, Host, HostRef, Opened, Process, Spawn, Spawned, StdHost, StderrMode, StdinSlot,
    Stream, WriteMode, exit_code,
};

/// 書いたバイト列を覚える書き込み先。テストが出力を読むために使う。
#[derive(Clone, Default)]
pub struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    pub fn contents(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8(bytes.clone()).expect("the output is UTF-8")
    }
}

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// `CaptureHost` が書いたファイル。パスの順に並ぶ。中身は UTF-8 とは限らない。
#[derive(Clone, Default)]
pub struct Written(Arc<Mutex<BTreeMap<String, Vec<u8>>>>);

impl Written {
    pub fn files(&self) -> Vec<(String, Vec<u8>)> {
        let files = self.lock();
        files
            .iter()
            .map(|(path, bytes)| (path.clone(), bytes.clone()))
            .collect()
    }

    fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.lock().get(path).cloned()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Vec<u8>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// `CaptureHost::open_write` が返す書き手。書き込みごとに表の lock を1つだけ取り、`path` の中身に足す。
struct WrittenFile {
    files: Written,
    path: String,
}

impl Write for WrittenFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.files
            .lock()
            .entry(self.path.clone())
            .or_default()
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// テストの Host。標準出力と標準エラーを、それぞれメモリに捕まえる。引数、標準入力、環境変数は与えたものだけで、本物の
/// 環境は読まない。テストの結果を実行する場所によらないものにするためである。`FileOut` はディスクに書かずに覚え、同じ
/// パスの `FileIn.open` はそれを読む。リポジトリの中にファイルを作らないためである。子のプロセスは本物を起動し、その
/// 標準出力と標準エラーも捕まえる (docs/implementation/testing.md の「UI テスト」)。
pub struct CaptureHost {
    // 出力とファイルの読みは `eml run` と同じ `StdHost` に任せ、出力のまとめ方も同じにする
    inner: StdHost,
    out: Capture,
    err: Capture,
    written: Written,
    file_root: PathBuf,
    args: Vec<OsString>,
    env: Vec<(String, OsString)>,
    /// `None` なら標準入力を使えない (REPL)。
    stdin: Option<Arc<StdinSlot>>,
    /// 子の `PATH` の先頭に足すディレクトリ。
    path_prefix: Option<PathBuf>,
}

impl CaptureHost {
    /// 引数なし、空の標準入力、空の環境で始める。`file_root` は `FileIn.open` の相対パスと子の作業ディレクトリの基準である。
    pub fn new(file_root: PathBuf) -> CaptureHost {
        let (out, err) = (Capture::default(), Capture::default());
        let inner = StdHost::new(
            Box::new(out.clone()),
            Box::new(err.clone()),
            file_root.clone(),
            Buffering::Block,
        );
        CaptureHost {
            inner,
            out,
            err,
            written: Written::default(),
            file_root,
            args: Vec::new(),
            env: Vec::new(),
            stdin: None,
            path_prefix: None,
        }
        .with_stdin(Vec::new())
    }

    pub fn with_args(mut self, args: Vec<OsString>) -> CaptureHost {
        self.args = args;
        self
    }

    pub fn with_stdin(mut self, bytes: Vec<u8>) -> CaptureHost {
        self.stdin = Some(Arc::new(StdinSlot::new(Box::new(Cursor::new(bytes)))));
        self
    }

    /// 標準入力を使えなくする。読むと `not available` の誤りになる。
    pub fn without_stdin(mut self) -> CaptureHost {
        self.stdin = None;
        self
    }

    pub fn with_env(mut self, env: Vec<(String, OsString)>) -> CaptureHost {
        self.env = env;
        self
    }

    /// 子の `PATH` の先頭に `dir` を足す。プログラムの `Env.get` が見る環境は変えない。
    pub fn with_path_prefix(mut self, dir: PathBuf) -> CaptureHost {
        self.path_prefix = Some(dir);
        self
    }

    pub fn output(&self) -> Capture {
        self.out.clone()
    }

    pub fn errors(&self) -> Capture {
        self.err.clone()
    }

    pub fn written(&self) -> Written {
        self.written.clone()
    }

    pub fn into_ref(self) -> HostRef {
        HostRef::new(Arc::new(self))
    }
}

impl Host for CaptureHost {
    fn write_line(&self, stream: Stream, line: &str) -> io::Result<()> {
        self.inner.write_line(stream, line)
    }

    fn write(&self, stream: Stream, bytes: &[u8]) -> io::Result<()> {
        self.inner.write(stream, bytes)
    }

    fn flush(&self) -> io::Result<()> {
        self.inner.flush()
    }

    fn open(&self, path: &str) -> io::Result<Opened> {
        match self.written.get(path) {
            Some(bytes) => {
                let size = bytes.len() as u64;
                Ok(Opened {
                    reader: Box::new(Cursor::new(bytes)),
                    size: Some(size),
                })
            }
            None => self.inner.open(path),
        }
    }

    fn args(&self) -> &[OsString] {
        &self.args
    }

    fn env_var(&self, name: &str) -> Option<OsString> {
        self.env
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    }

    fn stdin(&self) -> Option<Arc<StdinSlot>> {
        self.stdin.clone()
    }

    /// `Truncate` は開いた時点でそのパスを空にし、`Append` は今の中身を残す。
    fn open_write(&self, path: &str, mode: WriteMode) -> io::Result<Box<dyn Write + Send>> {
        let mut files = self.written.lock();
        let bytes = files.entry(path.to_string()).or_default();
        if mode == WriteMode::Truncate {
            bytes.clear();
        }
        Ok(Box::new(WrittenFile {
            files: self.written.clone(),
            path: path.to_string(),
        }))
    }

    /// 子の環境は、与えた表、本物の `PATH` (`with_path_prefix` のディレクトリが前に付く)、Windows では本物の
    /// `SystemRoot`、`Proc.env` の組をこの順に足したもので、`Stdio::Inherit` の標準入力は空である。受け継ぐ標準出力と
    /// 標準エラーは pipe にしてスレッドで最後まで読み、子を待った後で、捕まえたそれぞれの出力に `out`、`err` の順に
    /// 足す。捕まえた出力の順を固定するためである。
    /// スレッドで読むのは、子が pipe の buffer より多く書いても、eml の側の読み書きと詰まらないためである。
    fn spawn(&self, spawn: &Spawn<'_>) -> io::Result<Spawned> {
        self.inner.flush()?;
        let dir = spawn.dir(&self.file_root);
        let mut command = Command::new(spawn.program_path(&dir)?);
        command
            .args(spawn.args)
            .env_clear()
            .envs(self.env.iter().map(|(key, value)| (key, value)));
        if let Some(path) = child_path(self.path_prefix.as_deref())? {
            command.env("PATH", path);
        }
        // 子の Windows の初期化は `SystemRoot` を読む。環境を空にしたので、本物の値を渡さないと起動の途中で落ちる
        #[cfg(windows)]
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
        command.envs(spawn.env.iter().copied());
        if !dir.as_os_str().is_empty() {
            command.current_dir(&dir);
        }
        command.stdin(match spawn.stdin {
            eml_rt::Stdio::Pipe => Stdio::piped(),
            eml_rt::Stdio::Inherit | eml_rt::Stdio::Null => Stdio::null(),
        });
        let mut stdout = None;
        let mut out = None;
        match spawn.stdout {
            eml_rt::Stdio::Null => {
                command.stdout(Stdio::null());
                if spawn.stderr == StderrMode::ToStdout {
                    command.stderr(Stdio::null());
                }
            }
            pipe => {
                let (reader, writer) = io::pipe()?;
                if spawn.stderr == StderrMode::ToStdout {
                    command.stderr(writer.try_clone()?);
                }
                command.stdout(writer);
                if pipe == eml_rt::Stdio::Pipe {
                    stdout = Some(Box::new(reader) as Box<dyn Read + Send>);
                } else {
                    out = Some(reader);
                }
            }
        }
        let mut err = None;
        match spawn.stderr {
            StderrMode::Inherit => {
                let (reader, writer) = io::pipe()?;
                command.stderr(writer);
                err = Some(reader);
            }
            StderrMode::Null => {
                command.stderr(Stdio::null());
            }
            StderrMode::ToStdout => {}
        }
        let mut child = command.spawn()?;
        // `command` が持つ pipe の書き口を閉じる。残すと、子が終わっても読むスレッドに終わりが届かない
        drop(command);
        let stdin = child
            .stdin
            .take()
            .map(|w| Box::new(w) as Box<dyn Write + Send>);
        Ok(Spawned {
            stdin,
            stdout,
            process: Box::new(CapturedChild {
                child,
                out: out.map(drain),
                err: err.map(drain),
                out_capture: self.out.clone(),
                err_capture: self.err.clone(),
            }),
        })
    }
}

/// 子の `PATH`。`prefix` があれば本物の `PATH` の前に足す。`prefix` が区切りや `"` を含むと `join_paths` が
/// 失敗し、そのまま起動の誤りになる。
fn child_path(prefix: Option<&Path>) -> io::Result<Option<OsString>> {
    join_child_path(prefix, std::env::var_os("PATH"))
}

/// `child_path` の中身。本物の `PATH` を引数で受けるので、環境を変えずに試せる。
fn join_child_path(prefix: Option<&Path>, real: Option<OsString>) -> io::Result<Option<OsString>> {
    let Some(prefix) = prefix else {
        return Ok(real);
    };
    // 空の `PATH` を `split_paths` に通すと空のディレクトリが1つ出て、末尾に区切りが付くので除く
    let rest = real.iter().filter(|path| !path.is_empty());
    let dirs = std::iter::once(prefix.to_path_buf()).chain(rest.flat_map(std::env::split_paths));
    std::env::join_paths(dirs)
        .map(Some)
        .map_err(io::Error::other)
}

/// `pipe` を終わりまで読むスレッド。読みの誤りは、それまでに読んだ分で終える。
fn drain(mut pipe: PipeReader) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        bytes
    })
}

/// `CaptureHost::spawn` の子。待ったときに、読むスレッドが読んだ出力を捕まえた出力に足す。
struct CapturedChild {
    child: process::Child,
    out: Option<JoinHandle<Vec<u8>>>,
    err: Option<JoinHandle<Vec<u8>>>,
    out_capture: Capture,
    err_capture: Capture,
}

impl Process for CapturedChild {
    fn wait(&mut self) -> io::Result<i32> {
        let status = self.child.wait()?;
        for (thread, capture) in [
            (self.out.take(), &self.out_capture),
            (self.err.take(), &self.err_capture),
        ] {
            if let Some(thread) = thread {
                let bytes = thread.join().unwrap_or_default();
                capture.clone().write_all(&bytes)?;
            }
        }
        Ok(exit_code(status))
    }

    fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }
}

/// 標準出力をメモリに捕まえる Host。`CaptureHost::new` の既定 (引数なし、空の標準入力、空の環境) で作る。
pub fn capture_host(file_root: PathBuf) -> (HostRef, Capture) {
    let host = CaptureHost::new(file_root);
    let out = host.output();
    (host.into_ref(), out)
}

/// 標準出力と標準エラーに書く Host。引数と標準入力を持たないので、REPL が使う。
pub fn stdio_host(file_root: PathBuf) -> HostRef {
    HostRef::new(Arc::new(StdHost::stdio(file_root)))
}

/// `eml run` の Host。標準出力と標準エラーに書き、スクリプトに `args` を渡し、標準入力を読ませる。
pub fn script_host(file_root: PathBuf, args: Vec<OsString>) -> HostRef {
    let host = StdHost::stdio(file_root)
        .with_args(args)
        .with_stdin(Box::new(io::stdin()));
    HostRef::new(Arc::new(host))
}

#[cfg(all(test, unix))]
mod tests {
    use std::io::Read;
    use std::path::PathBuf;

    use eml_rt::{Host, Spawn, StderrMode, Stdio};

    use super::CaptureHost;

    fn spawn<'a>(script: &'a [&'a str], stdout: Stdio, stderr: StderrMode) -> Spawn<'a> {
        Spawn {
            program: "sh",
            args: script,
            env: &[],
            cwd: None,
            stdin: Stdio::Null,
            stdout,
            stderr,
        }
    }

    /// 受け継ぐ標準出力と標準エラーは、子を待った後で、捕まえた出力に `out`、`err` の順に足す。pipe の buffer より
    /// 多く書く子も、待つ前に詰まらない。
    #[test]
    fn a_child_is_captured_after_it_ends() {
        let host = CaptureHost::new(PathBuf::new());
        let script = [
            "-c",
            "head -c 100000 /dev/zero | tr '\\0' x; echo; echo err >&2",
        ];
        let mut child = host
            .spawn(&spawn(&script, Stdio::Inherit, StderrMode::Inherit))
            .unwrap();
        assert!(child.stdout.is_none());
        assert_eq!(child.process.wait().unwrap(), 0);
        assert_eq!(host.output().contents().len(), 100_001);
        assert_eq!(host.errors().contents(), "err\n");
    }

    /// `2>&1` は、捕まえる標準出力にも、eml に渡す pipe にも合流する。
    #[test]
    fn a_merged_error_joins_the_standard_output() {
        let host = CaptureHost::new(PathBuf::new());
        let script = ["-c", "echo out; echo err >&2"];
        let mut child = host
            .spawn(&spawn(&script, Stdio::Inherit, StderrMode::ToStdout))
            .unwrap();
        assert_eq!(child.process.wait().unwrap(), 0);
        assert_eq!(host.output().contents(), "out\nerr\n");
        let mut child = host
            .spawn(&spawn(&script, Stdio::Pipe, StderrMode::ToStdout))
            .unwrap();
        let mut read = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut read)
            .unwrap();
        assert_eq!(child.process.wait().unwrap(), 0);
        assert_eq!(read, "out\nerr\n");
        assert_eq!(host.output().contents(), "out\nerr\n");
        assert_eq!(host.errors().contents(), "");
    }
}

#[cfg(test)]
mod path_tests {
    use std::env::{join_paths, split_paths};
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use super::join_child_path;

    fn joined(dirs: &[&str]) -> OsString {
        join_paths(dirs.iter().map(PathBuf::from)).unwrap()
    }

    #[test]
    fn without_a_prefix_the_real_path_is_kept() {
        assert_eq!(join_child_path(None, None).unwrap(), None);
        let real = joined(&["a", "b"]);
        assert_eq!(
            join_child_path(None, Some(real.clone())).unwrap(),
            Some(real)
        );
        assert_eq!(
            join_child_path(None, Some(OsString::new())).unwrap(),
            Some(OsString::new())
        );
    }

    #[test]
    fn a_prefix_alone_becomes_the_path() {
        let d = Path::new("eml-bin");
        assert_eq!(
            join_child_path(Some(d), None).unwrap(),
            Some(joined(&["eml-bin"]))
        );
        assert_eq!(
            join_child_path(Some(d), Some(OsString::new())).unwrap(),
            Some(joined(&["eml-bin"]))
        );
    }

    #[test]
    fn a_prefix_goes_before_the_real_path() {
        let path = join_child_path(Some(Path::new("eml-bin")), Some(joined(&["a", "b"])))
            .unwrap()
            .unwrap();
        let dirs: Vec<PathBuf> = split_paths(&path).collect();
        assert_eq!(
            dirs,
            [
                PathBuf::from("eml-bin"),
                PathBuf::from("a"),
                PathBuf::from("b")
            ]
        );
    }

    #[test]
    fn a_prefix_with_a_separator_fails() {
        let separator = if cfg!(windows) { "eml;bin" } else { "eml:bin" };
        assert!(join_child_path(Some(Path::new(separator)), None).is_err());
    }
}
