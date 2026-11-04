//! VM を動かす Host (docs/spec/vm.md の「置き場所」)。`eml run` と REPL の Host と、テストが出力を読み、入力を与える
//! Host を置く。

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor, PipeReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::{self, JoinHandle};

use eml_rt::{
    Buffering, Host, HostRef, LaunchCause, LaunchError, NOT_REGULAR, OpenMode, Opened, OsHandle,
    Process, RawFile, RawProcess, SPAWN_LOCK, Spawn, Spawned, Stage, StdHost, StderrSpec,
    StdinSlot, StdinSpec, StdoutSpec, Stream, WriteMode, exit_code, here_file, raw_process,
    redirect_file, regular_file,
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

/// `CaptureHost` が書いたファイル。パスの順に並ぶ。中身は UTF-8 とは限らない。中身は一時ディレクトリの本物のファイルで、
/// 最後の写しを落とすと、ディレクトリごと消える (docs/implementation/testing.md の「UI テスト」)。`ui.rs` は Host を
/// 落とした後で読むので、Host ではなくこの写しが寿命を決める。
#[derive(Clone, Default)]
pub struct Written(Arc<Store>);

#[derive(Default)]
struct Store {
    /// パスから、`dir` の下のファイルの番号への表。パスは `/` や無い親ディレクトリを含みうるので、平らな番号で置く。
    files: Mutex<BTreeMap<String, usize>>,
    /// 最初の書き込みで作る。何も書かない run がディスクに触れないようにするためである。
    dir: OnceLock<PathBuf>,
    next: AtomicUsize,
}

impl Written {
    pub fn files(&self) -> Vec<(String, Vec<u8>)> {
        let files = self.0.lock();
        files
            .iter()
            .map(|(path, &n)| {
                let bytes = fs::read(self.0.file(n)).expect("a written file is readable");
                (path.clone(), bytes)
            })
            .collect()
    }

    /// 書いたファイルを読み直す。なければ `None`。
    fn open(&self, path: &str) -> io::Result<Option<(File, u64)>> {
        let files = self.0.lock();
        let Some(&n) = files.get(path) else {
            return Ok(None);
        };
        let file = File::open(self.0.file(n))?;
        let size = file.metadata()?.len();
        Ok(Some((file, size)))
    }

    /// `Truncate` はそのパスを空にし、`Append` は今の中身を残す。
    fn open_write(&self, path: &str, mode: WriteMode) -> io::Result<File> {
        let mut files = self.0.lock();
        self.0.make_dir()?;
        let mut options = OpenOptions::new();
        options.create(true);
        match mode {
            WriteMode::Truncate => options.write(true).truncate(true),
            WriteMode::Append => options.append(true),
        };
        if let Some(&n) = files.get(path) {
            return options.open(self.0.file(n));
        }
        let n = self.0.next.fetch_add(1, Ordering::Relaxed);
        let file = options.open(self.0.file(n))?;
        files.insert(path.to_string(), n);
        Ok(file)
    }
}

impl Store {
    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, usize>> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `make_dir` の後でだけ呼ぶ。
    fn file(&self, n: usize) -> PathBuf {
        let dir = self.dir.get().expect("the directory is made before a file");
        dir.join(n.to_string())
    }

    /// 呼び手が `files` の lock を持つので、2つのスレッドが同時に作ることはない。
    fn make_dir(&self) -> io::Result<()> {
        if self.dir.get().is_some() {
            return Ok(());
        }
        static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);
        let pid = process::id();
        let dir = loop {
            let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("eml-written-{pid}-{n}"));
            match fs::create_dir(&dir) {
                Ok(()) => break dir,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        };
        let _ = self.dir.set(dir);
        Ok(())
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Some(dir) = self.dir.get() {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

/// テストの Host。標準出力と標準エラーを、それぞれメモリに捕まえる。引数、標準入力、環境変数は与えたものだけで、本物の
/// 環境は読まない。テストの結果を実行する場所によらないものにするためである。`FileOut` は一時ディレクトリに書き、同じ
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
        match self.written.open(path)? {
            Some((file, size)) => Ok(Opened {
                reader: Box::new(file),
                size: Some(size),
            }),
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

    fn open_write(&self, path: &str, mode: WriteMode) -> io::Result<Box<dyn Write + Send>> {
        Ok(Box::new(self.written.open_write(path, mode)?))
    }

    /// `FileIn.open` と `FileOut` と同じ置き場を使う。読む向きは、このテストの Host が書いたファイルを先に引き、なければ
    /// `file_root` の下を開く。書く向きは一時ディレクトリのファイルを開き、`--- written:` に加わる。あるのに通常のファイルで
    /// ないものは表に入れずに断る (docs/implementation/testing.md の「UI テスト」、docs/spec/stdlib.md の「`Std.Stream`」)。
    fn open_raw(&self, path: &str, mode: OpenMode) -> io::Result<RawFile> {
        let write = match mode {
            OpenMode::Read => {
                return match self.written.open(path)? {
                    Some((file, _)) => regular_file(file),
                    None => self.inner.open_raw(path, mode),
                };
            }
            OpenMode::Create => WriteMode::Truncate,
            OpenMode::Append => WriteMode::Append,
        };
        if fs::metadata(self.file_root.join(path)).is_ok_and(|m| !m.is_file()) {
            return Err(io::Error::new(io::ErrorKind::Unsupported, NOT_REGULAR));
        }
        regular_file(self.written.open_write(path, write)?)
    }

    /// 子の環境は、与えた表、本物の `PATH` (`with_path_prefix` のディレクトリが前に付く)、Windows では本物の
    /// `SystemRoot`、段の `Proc.env` の組をこの順に足したもので、`StdinSpec::Inherit` の標準入力は空である。どの段の
    /// 受け継ぐ標準エラーも、最後の段の受け継ぐ標準出力も、pipe にしてスレッドで最後まで読み、段を待った後で、捕まえた
    /// それぞれの出力に `out`、`err` の順に足す。捕まえた出力の順を固定するためである。
    /// スレッドで読むのは、子が pipe の buffer より多く書いても、eml の側の読み書きと詰まらないためである。リダイレクトの
    /// パスは、このテストの Host が書いたファイルを先に引き、なければ `file_root` の下を開く。`> out` は書いたファイルに
    /// 加わる。`Given` の端は子に渡すだけで、スレッドで読まない (docs/spec/stdlib.md の「`Std.Exec`」)。
    fn spawn(&self, spawn: Spawn<'_>) -> io::Result<Spawned> {
        let last = spawn
            .stages
            .len()
            .checked_sub(1)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        self.inner.flush()?;
        let mut commands = Vec::with_capacity(spawn.stages.len());
        for (k, stage) in spawn.stages.iter().enumerate() {
            commands.push(
                self.command(stage)
                    .map_err(|e| LaunchError::spawning(k, e))?,
            );
        }
        // 子に渡すファイルは、どれも `SPAWN_LOCK` の前に開く。書いたファイルの表は自分の lock を取り、葉の lock の中では
        // 取れないためである。std のファイルは作ったときから子に受け継がれない (docs/spec/object-model.md の
        // 「子のプロセス」)
        let mut stdin = Some(match spawn.stdin {
            StdinSpec::Pipe => Stdio::piped(),
            StdinSpec::Inherit | StdinSpec::Null => Stdio::null(),
            StdinSpec::Given(handle) => handle.into(),
            StdinSpec::Text(bytes) => here_file(bytes)?.into(),
            StdinSpec::File(path) => self.redirect(0, path, None)?.into(),
        });
        let mut last_out = Some(match spawn.stdout {
            StdoutSpec::File { path, mode } => {
                LastOut::File(self.redirect(last, path, Some(mode))?)
            }
            StdoutSpec::Given(handle) => LastOut::Given(handle),
            StdoutSpec::Null => LastOut::Null,
            StdoutSpec::Pipe => LastOut::Pipe,
            StdoutSpec::Inherit => LastOut::Capture,
        });
        // 読むスレッドは放した後で作る
        let spawning = SPAWN_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let mut started: Vec<Started> = Vec::with_capacity(commands.len());
        let mut next_stdin: Option<PipeReader> = None;
        let mut stdout = None;
        let mut failed = None;
        for (k, (stage, mut command)) in spawn.stages.iter().zip(commands).enumerate() {
            let mut out = None;
            let mut err = None;
            let child = (|| {
                command.stdin(match next_stdin.take() {
                    Some(reader) => Stdio::from(reader),
                    None => stdin.take().unwrap_or_else(Stdio::null),
                });
                if k < last {
                    let (reader, writer) = io::pipe()?;
                    if stage.stderr == StderrSpec::ToStdout {
                        command.stderr(writer.try_clone()?);
                    }
                    command.stdout(writer);
                    next_stdin = Some(reader);
                } else {
                    match last_out.take().unwrap_or(LastOut::Null) {
                        LastOut::Null => {
                            command.stdout(Stdio::null());
                            if stage.stderr == StderrSpec::ToStdout {
                                command.stderr(Stdio::null());
                            }
                        }
                        LastOut::File(file) => {
                            if stage.stderr == StderrSpec::ToStdout {
                                command.stderr(file.try_clone()?);
                            }
                            command.stdout(file);
                        }
                        LastOut::Given(handle) => {
                            if stage.stderr == StderrSpec::ToStdout {
                                command.stderr(handle.try_clone()?);
                            }
                            command.stdout(handle);
                        }
                        pipe @ (LastOut::Pipe | LastOut::Capture) => {
                            let (reader, writer) = io::pipe()?;
                            if stage.stderr == StderrSpec::ToStdout {
                                command.stderr(writer.try_clone()?);
                            }
                            command.stdout(writer);
                            if matches!(pipe, LastOut::Pipe) {
                                stdout = Some(Box::new(reader) as Box<dyn Read + Send>);
                            } else {
                                out = Some(reader);
                            }
                        }
                    }
                }
                match stage.stderr {
                    StderrSpec::Inherit => {
                        let (reader, writer) = io::pipe()?;
                        command.stderr(writer);
                        err = Some(reader);
                    }
                    StderrSpec::Null => {
                        command.stderr(Stdio::null());
                    }
                    StderrSpec::ToStdout => {}
                }
                command.spawn()
            })();
            // `command` が持つ pipe の書き込む端を閉じる。残すと、子が終わっても読むスレッドと次の段に終わりが届かない
            drop(command);
            match child {
                Ok(child) => started.push(Started { child, out, err }),
                Err(error) => {
                    failed = Some(LaunchError::spawning(k, error));
                    break;
                }
            }
        }
        drop(next_stdin);
        drop(spawning);
        if let Some(error) = failed {
            kill_all(&mut started);
            return Err(error);
        }
        let mut drained = Vec::with_capacity(started.len());
        let mut failed = None;
        for stage in &mut started {
            let out = stage.out.take().map(drain).transpose();
            let err = stage.err.take().map(drain).transpose();
            match (out, err) {
                (Ok(out), Ok(err)) => drained.push((out, err)),
                (Err(error), _) | (_, Err(error)) => {
                    failed = Some(error);
                    break;
                }
            }
        }
        // 読むスレッドがないと捕まえる出力が届かないので、段を残さずに起動の誤りにする
        if let Some(error) = failed {
            kill_all(&mut started);
            return Err(error);
        }
        let stdin = started[0]
            .child
            .stdin
            .take()
            .map(|w| Box::new(w) as Box<dyn Write + Send>);
        let processes = started
            .into_iter()
            .zip(drained)
            .map(|(Started { child, .. }, (out, err))| {
                Box::new(CapturedChild {
                    child,
                    out,
                    err,
                    out_capture: self.out.clone(),
                    err_capture: self.err.clone(),
                }) as Box<dyn Process>
            })
            .collect();
        Ok(Spawned {
            stdin,
            stdout,
            processes,
        })
    }
}

/// `CaptureHost::spawn` の最後の段の標準出力。ファイルは `SPAWN_LOCK` の前に開いたものである。
enum LastOut {
    Null,
    File(File),
    Given(OsHandle),
    /// pipe にして eml に渡す。
    Pipe,
    /// pipe にしてスレッドで読み、捕まえた出力に足す。
    Capture,
}

/// `CaptureHost::spawn` が起動した段と、読むスレッドをまだ作っていない pipe。
struct Started {
    child: process::Child,
    out: Option<PipeReader>,
    err: Option<PipeReader>,
}

/// 起動した段をすべて kill してから待つ。起動の誤りで、段を残さないためである。
fn kill_all(started: &mut [Started]) {
    for stage in started.iter_mut() {
        let _ = stage.child.kill();
    }
    for stage in started.iter_mut() {
        let _ = stage.child.wait();
    }
}

impl CaptureHost {
    /// 段 `stage` の `Command`。標準入出力のほかを設定する。
    fn command(&self, stage: &Stage<'_>) -> io::Result<Command> {
        let dir = stage.dir(&self.file_root);
        let mut command = Command::new(stage.program_path(&dir)?);
        command
            .args(stage.args)
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
        command.envs(stage.env.iter().copied());
        if !dir.as_os_str().is_empty() {
            command.current_dir(&dir);
        }
        Ok(command)
    }

    /// 段 `stage` のリダイレクトのファイルを開く。`mode` が `None` なら読むために、`Some` なら書くために開く。
    fn redirect(&self, stage: usize, path: &str, mode: Option<WriteMode>) -> io::Result<File> {
        let opened = match mode {
            None => match self.written.open(path) {
                Ok(Some((file, _))) => Ok(file),
                Ok(None) => redirect_file(&self.file_root, path, None),
                Err(error) => Err(error),
            },
            Some(mode) => self.written.open_write(path, mode),
        };
        opened.map_err(|error| {
            LaunchError {
                stage,
                cause: LaunchCause::Open(path.into()),
            }
            .wrap(error)
        })
    }
}

/// 子の `PATH`。`prefix` があれば本物の `PATH` の前に足す。`prefix` が `join_paths` の断る文字 (unix では区切りの
/// `:`、Windows では `"`) を含むと失敗し、そのまま起動の誤りになる。
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

/// `pipe` を終わりまで読むスレッド。読みの誤りは、それまでに読んだ分で終える。SIGINT と SIGTERM を塞いで始め、2つを
/// worker 0 に届ける (docs/spec/vm.md の「上限と割り込み」)。
fn drain(mut pipe: PipeReader) -> io::Result<JoinHandle<Vec<u8>>> {
    let builder = thread::Builder::new().name("eml-drain".into());
    eml_io::spawn_masked(builder, move || {
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

    /// 本物の時計の UI テストでも `Std.Exec.wait` が driver の待ちを通るように、OS の子を渡す。
    fn raw(&self) -> Option<RawProcess> {
        raw_process(&self.child)
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

    use eml_rt::{Host, Spawn, Spawned, Stage, StderrSpec, StdinSpec, StdoutSpec};

    use super::CaptureHost;

    fn spawn(
        host: &CaptureHost,
        script: &[&str],
        stdout: StdoutSpec,
        stderr: StderrSpec,
    ) -> Spawned {
        let stage = Stage {
            program: "sh",
            args: script,
            env: &[],
            cwd: None,
            stderr,
        };
        host.spawn(Spawn {
            stages: &[stage],
            stdin: StdinSpec::Null,
            stdout,
        })
        .unwrap()
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
        let mut child = spawn(&host, &script, StdoutSpec::Inherit, StderrSpec::Inherit);
        assert!(child.stdout.is_none());
        assert_eq!(child.processes[0].wait().unwrap(), 0);
        assert_eq!(host.output().contents().len(), 100_001);
        assert_eq!(host.errors().contents(), "err\n");
    }

    /// `2>&1` は、捕まえる標準出力にも、eml に渡す pipe にも合流する。
    #[test]
    fn a_merged_error_joins_the_standard_output() {
        let host = CaptureHost::new(PathBuf::new());
        let script = ["-c", "echo out; echo err >&2"];
        let mut child = spawn(&host, &script, StdoutSpec::Inherit, StderrSpec::ToStdout);
        assert_eq!(child.processes[0].wait().unwrap(), 0);
        assert_eq!(host.output().contents(), "out\nerr\n");
        let mut child = spawn(&host, &script, StdoutSpec::Pipe, StderrSpec::ToStdout);
        let mut read = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut read)
            .unwrap();
        assert_eq!(child.processes[0].wait().unwrap(), 0);
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
    fn a_prefix_join_paths_cannot_hold_fails() {
        // unix の `join_paths` は区切りの `:` を、Windows の `join_paths` は `"` を含む項を断る。Windows は `;` を
        // 含む項を `"` で囲んで受け入れる
        let prefix = if cfg!(windows) { "eml\"bin" } else { "eml:bin" };
        assert!(join_child_path(Some(Path::new(prefix)), None).is_err());
    }
}

#[cfg(test)]
mod store_tests {
    use std::fs;
    use std::io::{Read, Write};
    use std::path::PathBuf;

    use eml_rt::{Host, WriteMode};

    use super::CaptureHost;

    fn write(host: &CaptureHost, path: &str, mode: WriteMode, bytes: &[u8]) {
        let mut file = host.open_write(path, mode).unwrap();
        file.write_all(bytes).unwrap();
    }

    #[test]
    fn written_files_keep_the_path_order() {
        let host = CaptureHost::new(PathBuf::new());
        write(&host, "b", WriteMode::Truncate, b"2");
        write(&host, "a", WriteMode::Truncate, b"1");
        write(&host, "c", WriteMode::Truncate, b"3");
        let paths: Vec<String> = host.written().files().into_iter().map(|f| f.0).collect();
        assert_eq!(paths, ["a", "b", "c"]);
    }

    #[test]
    fn a_path_with_missing_directories_is_written() {
        let host = CaptureHost::new(PathBuf::new());
        write(&host, "no/such/dir/out.txt", WriteMode::Truncate, b"x");
        assert_eq!(
            host.written().files(),
            [("no/such/dir/out.txt".to_string(), b"x".to_vec())]
        );
    }

    #[test]
    fn a_truncate_after_an_append_starts_empty() {
        let host = CaptureHost::new(PathBuf::new());
        write(&host, "f", WriteMode::Append, b"one");
        write(&host, "f", WriteMode::Append, b"two");
        assert_eq!(host.written().files()[0].1, b"onetwo");
        write(&host, "f", WriteMode::Truncate, b"");
        assert_eq!(host.written().files()[0].1, b"");
    }

    #[test]
    fn a_written_file_is_read_back_by_open() {
        let host = CaptureHost::new(PathBuf::new());
        write(&host, "f", WriteMode::Truncate, b"hello");
        let opened = host.open("f").unwrap();
        assert_eq!(opened.size, Some(5));
        let mut read = String::new();
        let mut reader = opened.reader;
        reader.read_to_string(&mut read).unwrap();
        assert_eq!(read, "hello");
    }

    #[test]
    fn the_directory_is_made_on_the_first_write_only() {
        let host = CaptureHost::new(PathBuf::new());
        let written = host.written();
        assert!(written.0.dir.get().is_none());
        write(&host, "f", WriteMode::Truncate, b"x");
        assert!(written.0.dir.get().unwrap().is_dir());
    }

    #[test]
    fn the_directory_is_removed_when_the_last_view_drops() {
        let host = CaptureHost::new(PathBuf::new());
        write(&host, "f", WriteMode::Truncate, b"x");
        let written = host.written();
        let dir = written.0.dir.get().unwrap().clone();
        drop(host);
        assert!(dir.is_dir());
        drop(written);
        assert!(!fs::exists(&dir).unwrap());
    }
}
