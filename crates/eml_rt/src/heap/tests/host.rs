//! テストの Host。出力をメモリに受け、ファイルをメモリから読ませる。Miri でも動くように、OS に触れない。

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, Cursor, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::{
    Host, HostRef, LaunchError, Opened, Process, Spawn, Spawned, StderrSpec, StdinSlot, StdinSpec,
    StdoutSpec, Stream, WriteMode,
};

/// メモリの上のファイル。読み手の振る舞い (1回に返すバイト数、途中の誤り、中断) を決められる。
#[derive(Clone, Default)]
pub(super) struct MemoryFile {
    pub(super) bytes: Vec<u8>,
    /// 読む前に分かる長さとして Host が返すか
    pub(super) sized: bool,
    /// 読む前に分かる長さとして、中身の長さの代わりに返す値
    pub(super) claimed_size: Option<u64>,
    /// 1回の読みで返す最大のバイト数。0 なら制限しない
    pub(super) chunk: usize,
    /// このバイト数を読んだ後の読みで誤りを返す
    pub(super) fail_after: Option<usize>,
    /// 最初の読みで `Interrupted` を1回返す
    pub(super) interrupt_once: bool,
    /// 最初の読みで誤りを1回返す
    pub(super) fail_once: bool,
    /// 最初の読みでこの `Control` に割り込みを立て、`Interrupted` を返す。SIGINT のハンドラが止める語を立てた後に、
    /// 待っている読みが `EINTR` で戻ることを表す。その後にもう一度読むと読み手が panic するので、すぐ止めなければ
    /// テストが落ちる
    pub(super) interrupt_with: Option<Arc<crate::Control>>,
    /// 読み手が落ちた回数
    pub(super) drops: Option<Arc<AtomicUsize>>,
    /// 読み手の `read` が呼ばれた回数
    pub(super) reads: Option<Arc<AtomicUsize>>,
}

impl MemoryFile {
    pub(super) fn text(s: &str) -> MemoryFile {
        MemoryFile {
            bytes: s.as_bytes().to_vec(),
            sized: true,
            ..MemoryFile::default()
        }
    }
}

struct Reader {
    file: MemoryFile,
    at: usize,
    interrupted: bool,
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(reads) = &self.file.reads {
            reads.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(control) = &self.file.interrupt_with {
            assert!(!self.interrupted, "the read was retried after an interrupt");
            self.interrupted = true;
            control.interrupt();
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        if self.file.interrupt_once && !self.interrupted {
            self.interrupted = true;
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        if self.file.fail_once && !self.interrupted {
            self.interrupted = true;
            return Err(io::Error::other("the disk went away once"));
        }
        if self.file.fail_after.is_some_and(|n| self.at >= n) {
            return Err(io::Error::other("the disk went away"));
        }
        let rest = &self.file.bytes[self.at..];
        let mut n = rest.len().min(buf.len());
        if self.file.chunk > 0 {
            n = n.min(self.file.chunk);
        }
        buf[..n].copy_from_slice(&rest[..n]);
        self.at += n;
        Ok(n)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        if let Some(drops) = &self.file.drops {
            drops.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[derive(Default)]
pub(super) struct MemoryHost {
    out: Mutex<Vec<u8>>,
    err: Mutex<Vec<u8>>,
    files: Mutex<HashMap<String, MemoryFile>>,
    /// 書き込みで返す誤りの文言
    fail_write: Option<&'static str>,
    /// `flush` で返す誤りの文言
    fail_flush: Option<&'static str>,
    args: Vec<OsString>,
    env: HashMap<String, OsString>,
    /// `None` なら標準入力を使えない
    stdin: Option<Arc<StdinSlot>>,
    /// `open_write` の書き手が書いたファイル
    written: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    /// `open_write` の書き手が受けたバイト数。書けなかったバイトも数える
    received: Arc<AtomicUsize>,
    /// `flush` を呼ばれた回数
    flushes: AtomicUsize,
    /// `spawn` の子の `wait` の中でこの `Control` に割り込みを立てる。子を待つ間に届いた SIGINT を表す
    interrupt_run: Option<Arc<crate::Control>>,
    /// `spawn` の子が標準出力に書くバイト列
    child_output: Vec<u8>,
    /// `spawn` が受けた起動
    spawns: Mutex<Vec<Started>>,
    /// `spawn` の子の標準入力に書かれたバイト列。書けなかったバイトは入らない
    child_input: Arc<Mutex<Vec<u8>>>,
    /// `spawn` の子の標準入力への最初の書き込みが、1回 `Interrupted` を返すか
    child_interrupts_once: bool,
    /// `spawn` の子の `wait` と `kill` が呼ばれた回数
    waits: Arc<AtomicUsize>,
    kills: Arc<AtomicUsize>,
}

/// `MemoryHost::spawn` が受けた起動の設定。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Started {
    pub(super) program: String,
    pub(super) args: Vec<String>,
    pub(super) env: Vec<(String, String)>,
    pub(super) cwd: Option<String>,
    pub(super) stdio: (Stdio, Stdio, Stderr),
}

/// `MemoryHost::spawn` が受ける標準入力と標準出力の向き。ファイルと端を使う向きは断るので、残りの3つである。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stdio {
    Inherit,
    Null,
    Pipe,
}

/// `MemoryHost::spawn` が受ける段の標準エラーの向き。端を使う向きは断るので、残りの3つである。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stderr {
    Inherit,
    ToStdout,
    Null,
}

impl MemoryHost {
    pub(super) fn failing(message: &'static str) -> MemoryHost {
        MemoryHost {
            fail_write: Some(message),
            ..MemoryHost::default()
        }
    }
    /// 書き込みは通り、`flush` だけが失敗する Host
    pub(super) fn failing_flush(message: &'static str) -> MemoryHost {
        MemoryHost {
            fail_flush: Some(message),
            ..MemoryHost::default()
        }
    }
    pub(super) fn add(&self, path: &str, file: MemoryFile) {
        self.files.lock().unwrap().insert(path.to_string(), file);
    }
    pub(super) fn out(&self) -> String {
        String::from_utf8(self.out.lock().unwrap().clone()).unwrap()
    }
    pub(super) fn err(&self) -> String {
        String::from_utf8(self.err.lock().unwrap().clone()).unwrap()
    }
    pub(super) fn out_bytes(&self) -> Vec<u8> {
        self.out.lock().unwrap().clone()
    }
    pub(super) fn err_bytes(&self) -> Vec<u8> {
        self.err.lock().unwrap().clone()
    }
    pub(super) fn with_args(mut self, args: Vec<OsString>) -> MemoryHost {
        self.args = args;
        self
    }
    pub(super) fn with_env(mut self, env: Vec<(&str, OsString)>) -> MemoryHost {
        self.env = env.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        self
    }
    /// 標準入力をメモリのファイルから読ませる
    pub(super) fn with_stdin(mut self, file: MemoryFile) -> MemoryHost {
        let reader = Box::new(Reader {
            file,
            at: 0,
            interrupted: false,
        });
        self.stdin = Some(Arc::new(StdinSlot::new(reader)));
        self
    }
    pub(super) fn interrupting(mut self, control: Arc<crate::Control>) -> MemoryHost {
        self.interrupt_run = Some(control);
        self
    }
    pub(super) fn written(&self, path: &str) -> Option<String> {
        self.written_bytes(path)
            .map(|bytes| String::from_utf8(bytes).unwrap())
    }
    pub(super) fn written_bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.written.lock().unwrap().get(path).cloned()
    }
    pub(super) fn received(&self) -> usize {
        self.received.load(Ordering::Relaxed)
    }
    pub(super) fn flushes(&self) -> usize {
        self.flushes.load(Ordering::Relaxed)
    }
    /// `spawn` の子が標準出力に書くバイト列
    pub(super) fn with_child_output(mut self, bytes: &[u8]) -> MemoryHost {
        self.child_output = bytes.to_vec();
        self
    }
    /// `spawn` の子の標準入力への最初の書き込みは、1回 `Interrupted` を返す。`interrupting` もあれば、その前に止める語を
    /// 立てる。待っている書き込みが SIGINT の後に `EINTR` で戻ることを表す
    pub(super) fn with_child_interrupted_once(mut self) -> MemoryHost {
        self.child_interrupts_once = true;
        self
    }
    pub(super) fn spawns(&self) -> Vec<Started> {
        self.spawns.lock().unwrap().clone()
    }
    pub(super) fn child_input(&self) -> Vec<u8> {
        self.child_input.lock().unwrap().clone()
    }
    /// `spawn` の子の `wait` と `kill` が呼ばれた回数
    pub(super) fn waits_and_kills(&self) -> (usize, usize) {
        (
            self.waits.load(Ordering::Relaxed),
            self.kills.load(Ordering::Relaxed),
        )
    }
}

impl Host for MemoryHost {
    fn write_line(&self, stream: Stream, line: &str) -> io::Result<()> {
        if let Some(message) = self.fail_write {
            return Err(io::Error::other(message));
        }
        let mut target = match stream {
            Stream::Out => self.out.lock().unwrap(),
            Stream::Err => self.err.lock().unwrap(),
        };
        target.extend_from_slice(line.as_bytes());
        target.push(b'\n');
        Ok(())
    }
    fn write(&self, stream: Stream, bytes: &[u8]) -> io::Result<()> {
        if let Some(message) = self.fail_write {
            return Err(io::Error::other(message));
        }
        let mut target = match stream {
            Stream::Out => self.out.lock().unwrap(),
            Stream::Err => self.err.lock().unwrap(),
        };
        target.extend_from_slice(bytes);
        Ok(())
    }
    fn flush(&self) -> io::Result<()> {
        self.flushes.fetch_add(1, Ordering::Relaxed);
        match self.fail_flush {
            Some(message) => Err(io::Error::other(message)),
            None => Ok(()),
        }
    }
    fn open(&self, path: &str) -> io::Result<Opened> {
        let file = self
            .files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        let size = file
            .claimed_size
            .or(file.sized.then_some(file.bytes.len() as u64));
        let reader = Box::new(Reader {
            file,
            at: 0,
            interrupted: false,
        });
        Ok(Opened { reader, size })
    }
    fn args(&self) -> &[OsString] {
        &self.args
    }
    fn env_var(&self, name: &str) -> Option<OsString> {
        self.env.get(name).cloned()
    }
    fn stdin(&self) -> Option<Arc<StdinSlot>> {
        self.stdin.clone()
    }
    /// 空のパスは開けない。`fail_write` があれば、書き手は書くたびにその文言で失敗する
    fn open_write(&self, path: &str, mode: WriteMode) -> io::Result<Box<dyn Write + Send>> {
        if path.is_empty() {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        let mut written = self.written.lock().unwrap();
        let bytes = written.entry(path.to_string()).or_default();
        if mode == WriteMode::Truncate {
            bytes.clear();
        }
        Ok(Box::new(Writer {
            written: self.written.clone(),
            path: path.to_string(),
            fail: self.fail_write,
            received: self.received.clone(),
        }))
    }
    /// プロセスを起動せず、段ごとに受けた設定を覚える。`missing` は見つからないプログラムで、それより前の段を kill して
    /// 待ってから、その段の誤りを返す。段の間は pipe とみなす。pipe の標準入力は `child_input` に写し (`fail_write`
    /// があれば書くたびにその文言で失敗する)、pipe の標準出力は `child_output` を読ませる。段の `wait` は引数の数を終了
    /// コードにする。ファイルを使う向き (文字列の標準入力とリダイレクト) は OS のファイルが要るので `Unsupported` で断る
    fn spawn(&self, spawn: Spawn<'_>) -> io::Result<Spawned> {
        let last = spawn
            .stages
            .len()
            .checked_sub(1)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let unsupported = || io::Error::from(io::ErrorKind::Unsupported);
        let stdin = match spawn.stdin {
            StdinSpec::Inherit => Stdio::Inherit,
            StdinSpec::Null => Stdio::Null,
            StdinSpec::Pipe => Stdio::Pipe,
            StdinSpec::Given(_) | StdinSpec::Text(_) | StdinSpec::File(_) => {
                return Err(unsupported());
            }
        };
        let stdout = match spawn.stdout {
            StdoutSpec::Inherit => Stdio::Inherit,
            StdoutSpec::Null => Stdio::Null,
            StdoutSpec::Pipe => Stdio::Pipe,
            StdoutSpec::Given(_) | StdoutSpec::File { .. } => return Err(unsupported()),
        };
        let stderrs = spawn
            .stages
            .iter()
            .map(|stage| match stage.stderr {
                StderrSpec::Inherit => Ok(Stderr::Inherit),
                StderrSpec::ToStdout => Ok(Stderr::ToStdout),
                StderrSpec::Null => Ok(Stderr::Null),
                StderrSpec::Given(_) => Err(unsupported()),
            })
            .collect::<io::Result<Vec<_>>>()?;
        let mut processes: Vec<Box<dyn Process>> = Vec::new();
        for (k, (stage, stderr)) in spawn.stages.iter().zip(stderrs).enumerate() {
            if stage.program == "missing" {
                for process in &mut processes {
                    process.kill()?;
                }
                for process in &mut processes {
                    process.wait()?;
                }
                return Err(LaunchError::spawning(
                    k,
                    io::Error::from(io::ErrorKind::NotFound),
                ));
            }
            self.spawns.lock().unwrap().push(Started {
                program: stage.program.to_string(),
                args: stage.args.iter().map(|a| a.to_string()).collect(),
                env: stage
                    .env
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                cwd: stage.cwd.map(str::to_string),
                stdio: (
                    if k == 0 { stdin } else { Stdio::Pipe },
                    if k == last { stdout } else { Stdio::Pipe },
                    stderr,
                ),
            });
            processes.push(Box::new(MemoryProcess {
                code: stage.args.len() as i32,
                waits: self.waits.clone(),
                kills: self.kills.clone(),
                interrupt: self.interrupt_run.clone(),
            }));
        }
        let stdin = (stdin == Stdio::Pipe).then(|| {
            Box::new(ChildInput {
                bytes: self.child_input.clone(),
                fail: self.fail_write,
                interrupt_once: self.child_interrupts_once,
                interrupt: self.interrupt_run.clone(),
            }) as Box<dyn Write + Send>
        });
        let stdout = (stdout == Stdio::Pipe)
            .then(|| Box::new(Cursor::new(self.child_output.clone())) as Box<dyn Read + Send>);
        Ok(Spawned {
            stdin,
            stdout,
            processes,
        })
    }
}

/// `MemoryHost::spawn` の子の標準入力。
struct ChildInput {
    bytes: Arc<Mutex<Vec<u8>>>,
    fail: Option<&'static str>,
    interrupt_once: bool,
    interrupt: Option<Arc<crate::Control>>,
}

impl Write for ChildInput {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if std::mem::take(&mut self.interrupt_once) {
            if let Some(control) = &self.interrupt {
                control.interrupt();
            }
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        if let Some(message) = self.fail {
            return Err(io::Error::other(message));
        }
        self.bytes.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// `MemoryHost::spawn` の子。呼ばれた `wait` と `kill` を数える。
struct MemoryProcess {
    code: i32,
    waits: Arc<AtomicUsize>,
    kills: Arc<AtomicUsize>,
    interrupt: Option<Arc<crate::Control>>,
}

impl Process for MemoryProcess {
    fn wait(&mut self) -> io::Result<i32> {
        self.waits.fetch_add(1, Ordering::Relaxed);
        if let Some(control) = &self.interrupt {
            control.interrupt();
        }
        Ok(self.code)
    }
    fn kill(&mut self) -> io::Result<()> {
        self.kills.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// `MemoryHost::open_write` の書き手。メモリの表の `path` の中身に足す。
struct Writer {
    written: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    path: String,
    fail: Option<&'static str>,
    received: Arc<AtomicUsize>,
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.received.fetch_add(buf.len(), Ordering::Relaxed);
        if let Some(message) = self.fail {
            return Err(io::Error::other(message));
        }
        let mut written = self.written.lock().unwrap();
        written
            .entry(self.path.clone())
            .or_default()
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// テストの `Ctx` に渡す Host
pub(super) fn host(memory: &Arc<MemoryHost>) -> HostRef {
    HostRef::new(memory.clone())
}

/// 書いた中身を外から見られる書き込み先。`StdHost` に渡す。2つ目の欄が立っていれば、次の書き込みで1回 panic する
#[derive(Clone, Default)]
pub(super) struct Shared(Arc<Mutex<Vec<u8>>>, Arc<Mutex<bool>>);

impl Shared {
    pub(super) fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // 旗のロックを放してから panic する。持ったままだと、旗のロックまで poisoned になって次の書き込みが止まる
        let panic_now = std::mem::take(&mut *self.1.lock().unwrap());
        if panic_now {
            panic!("a writer that panics once");
        }
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

mod std_host {
    use std::io::{self, Read, Write};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::path::PathBuf;

    use super::Shared;
    use crate::{Buffering, Control, Host, Limits, OpenMode, StdHost, Stream, WriteMode};

    fn std_host(mode: Buffering) -> (StdHost, Shared, Shared) {
        let (out, err) = (Shared::default(), Shared::default());
        let host = StdHost::new(
            Box::new(out.clone()),
            Box::new(err.clone()),
            PathBuf::new(),
            mode,
        );
        (host, out, err)
    }

    /// 設定できない名前は、`var_os` に渡さずに設定されていないとみなす。
    #[test]
    fn an_impossible_variable_name_is_unset() {
        let (host, _, _) = std_host(Buffering::Line);
        for name in ["", "A=B", "A\0B"] {
            assert_eq!(host.env_var(name), None, "{name:?}");
        }
    }

    #[test]
    fn a_host_without_a_standard_input_refuses_to_read_it() {
        let (host, _, _) = std_host(Buffering::Line);
        assert!(host.stdin().is_none());
        let host = host.with_stdin(Box::new(io::Cursor::new(b"x".to_vec())));
        let control = Control::new(Limits::default());
        let line = host
            .stdin()
            .unwrap()
            .take()
            .map(|mut r| r.fill_buf(&control, None).ok().map(<[u8]>::to_vec));
        assert_eq!(line, Some(Some(b"x".to_vec())));
    }

    /// `Truncate` は前の中身を消し、`Append` は残す。どちらも `file_root` の下のパスを開く。
    #[test]
    fn a_file_is_opened_for_writing_under_the_root() {
        let dir = std::env::temp_dir().join(format!("eml_rt-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("out.txt"), "old").unwrap();
        let host = StdHost::new(
            Box::new(io::sink()),
            Box::new(io::sink()),
            dir.clone(),
            Buffering::Line,
        );
        let write = |mode, text: &str| {
            let mut w = host.open_write("out.txt", mode).unwrap();
            w.write_all(text.as_bytes()).unwrap();
        };
        write(WriteMode::Truncate, "日本");
        assert_eq!(
            std::fs::read_to_string(dir.join("out.txt")).unwrap(),
            "日本"
        );
        write(WriteMode::Append, "語");
        assert_eq!(
            std::fs::read_to_string(dir.join("out.txt")).unwrap(),
            "日本語"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// pipe の子に書き、`2>&1` の1本の pipe から標準出力と標準エラーを順に読む。起動の前に標準出力を書き出す。
    /// kill した子はシグナルの番号の終了コードを返す。
    #[cfg(unix)]
    #[test]
    fn a_spawned_child_talks_through_pipes() {
        use crate::{Spawn, Stage, StderrSpec, StdinSpec, StdoutSpec};
        let (host, out, _) = std_host(Buffering::Block);
        host.write_line(Stream::Out, "before").unwrap();
        let stage = Stage {
            program: "sh",
            args: &["-c", "cat; echo err >&2; exit 3"],
            env: &[],
            cwd: None,
            stderr: StderrSpec::ToStdout,
        };
        let spawn = Spawn {
            stages: vec![stage],
            stdin: StdinSpec::Pipe,
            stdout: StdoutSpec::Pipe,
        };
        let mut child = host.spawn(spawn).unwrap();
        assert_eq!(out.text(), "before\n");
        child.stdin.take().unwrap().write_all(b"in\n").unwrap();
        let mut read = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut read)
            .unwrap();
        assert_eq!(read, "in\nerr\n");
        assert_eq!(child.processes[0].wait().unwrap(), 3);
        let sleeper = Stage {
            program: "sh",
            args: &["-c", "sleep 10"],
            env: &[],
            cwd: None,
            stderr: StderrSpec::Null,
        };
        let mut child = host
            .spawn(Spawn {
                stages: vec![sleeper],
                stdin: StdinSpec::Null,
                stdout: StdoutSpec::Null,
            })
            .unwrap();
        assert!(child.stdin.is_none() && child.stdout.is_none());
        child.processes[0].kill().unwrap();
        assert_eq!(child.processes[0].wait().unwrap(), 128 + 9);
        let missing = Stage {
            program: "eml-no-such-program",
            args: &["-c", "sleep 10"],
            env: &[],
            cwd: None,
            stderr: StderrSpec::Null,
        };
        let missing = Spawn {
            stages: vec![missing],
            stdin: StdinSpec::Null,
            stdout: StdoutSpec::Null,
        };
        assert_eq!(
            host.spawn(missing).err().map(|e| e.kind()),
            Some(io::ErrorKind::NotFound)
        );
    }

    /// 作業ディレクトリは `file_root` につなぎ、`/` を含む相対のプログラムはそこに対して解決する。環境変数は上書き
    /// する。
    #[cfg(unix)]
    #[test]
    fn a_spawned_child_runs_in_its_directory() {
        use std::os::unix::fs::PermissionsExt;

        use crate::{Spawn, Stage, StderrSpec, StdinSpec, StdoutSpec};
        let dir = std::env::temp_dir().join(format!("eml_rt-spawn-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let script = dir.join("sub").join("run.sh");
        std::fs::write(&script, "#!/bin/sh\necho \"$FOO $(cat hello.txt)\"\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.join("sub").join("hello.txt"), "hi").unwrap();
        let host = StdHost::new(
            Box::new(io::sink()),
            Box::new(io::sink()),
            dir.clone(),
            Buffering::Line,
        );
        let spawn = Spawn {
            stages: vec![Stage {
                program: "./run.sh",
                args: &[],
                env: &[("FOO", "a"), ("FOO", "b")],
                cwd: Some("sub"),
                stderr: StderrSpec::Inherit,
            }],
            stdin: StdinSpec::Null,
            stdout: StdoutSpec::Pipe,
        };
        let mut child = host.spawn(spawn).unwrap();
        let mut read = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut read)
            .unwrap();
        assert_eq!(read, "b hi\n");
        assert_eq!(child.processes[0].wait().unwrap(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_line_host_writes_each_line_at_once() {
        let (host, out, err) = std_host(Buffering::Line);
        host.write_line(Stream::Out, "a").unwrap();
        assert_eq!(out.text(), "a\n");
        host.write_line(Stream::Err, "e").unwrap();
        assert_eq!(err.text(), "e\n");
    }

    #[test]
    fn a_block_host_writes_on_flush() {
        let (host, out, _) = std_host(Buffering::Block);
        host.write_line(Stream::Out, "a").unwrap();
        assert_eq!(out.text(), "");
        host.flush().unwrap();
        assert_eq!(out.text(), "a\n");
    }

    /// 明示の flush なしで落としても、まだ書いていない出力は書かれる (unwind のときも同じ)。
    #[test]
    fn a_dropped_block_host_writes_what_it_holds() {
        let (host, out, _) = std_host(Buffering::Block);
        host.write_line(Stream::Out, "kept").unwrap();
        drop(host);
        assert_eq!(out.text(), "kept\n");
    }

    /// 標準エラーは、`Block` でも行ごとに書く。
    #[test]
    fn the_error_stream_is_written_by_line() {
        let (host, _, err) = std_host(Buffering::Block);
        host.write_line(Stream::Err, "e").unwrap();
        assert_eq!(err.text(), "e\n");
    }

    /// `Line` のバイト列の書き込みは、`\n` を含むときだけ書き出す。
    #[test]
    fn a_line_host_flushes_a_write_with_a_newline() {
        let (host, out, _) = std_host(Buffering::Line);
        host.write(Stream::Out, b"a").unwrap();
        assert_eq!(out.text(), "");
        host.write(Stream::Out, b"b\nc").unwrap();
        assert_eq!(out.text(), "ab\nc");
    }

    /// `Block` のバイト列の書き込みは、`\n` を含んでも書き出さない。
    #[test]
    fn a_block_host_holds_writes_until_flush() {
        let (host, out, _) = std_host(Buffering::Block);
        host.write(Stream::Out, b"a\n").unwrap();
        assert_eq!(out.text(), "");
        host.flush().unwrap();
        assert_eq!(out.text(), "a\n");
    }

    /// 標準エラーへのバイト列の書き込みは、`Block` でも、`\n` がなくても、書くたびに書き出す。
    #[test]
    fn the_error_stream_flushes_every_write() {
        for mode in [Buffering::Line, Buffering::Block] {
            let (host, _, err) = std_host(mode);
            host.write(Stream::Err, b"e").unwrap();
            assert_eq!(err.text(), "e");
        }
    }

    /// 書き込み先が panic してロックが poisoned になっても、次の行は書ける。panic した行は `BufWriter` に残り、次の行と一緒に
    /// 書かれる。
    #[test]
    fn a_poisoned_lock_is_recovered() {
        let (host, out, _) = std_host(Buffering::Line);
        *out.1.lock().unwrap() = true;
        let panicked = catch_unwind(AssertUnwindSafe(|| host.write_line(Stream::Out, "a")));
        assert!(panicked.is_err());
        host.write_line(Stream::Out, "b").unwrap();
        assert_eq!(out.text(), "a\nb\n");
    }

    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eml_rt-host-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn files_open_under_the_root_or_by_absolute_path() {
        let root = fixture("open");
        std::fs::write(root.join("a.txt"), "abc").unwrap();
        let host = StdHost::new(
            Box::new(io::sink()),
            Box::new(io::sink()),
            root.clone(),
            Buffering::Line,
        );
        let mut opened = host.open("a.txt").unwrap();
        assert_eq!(opened.size, Some(3));
        let mut text = String::new();
        opened.reader.read_to_string(&mut text).unwrap();
        assert_eq!(text, "abc");
        let absolute = root.join("a.txt");
        assert!(host.open(absolute.to_str().unwrap()).is_ok());
        assert_eq!(
            host.open("missing.txt").err().unwrap().kind(),
            io::ErrorKind::NotFound
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// ディレクトリは読むためにも書くためにも開かず、どの OS でも `IsADirectory` になる。
    #[test]
    fn a_directory_does_not_open() {
        let root = fixture("dir");
        let host = StdHost::new(
            Box::new(io::sink()),
            Box::new(io::sink()),
            root.clone(),
            Buffering::Line,
        );
        assert_eq!(
            host.open(".").err().map(|e| e.kind()),
            Some(io::ErrorKind::IsADirectory)
        );
        assert_eq!(
            host.open_write(".", WriteMode::Truncate)
                .err()
                .map(|e| e.kind()),
            Some(io::ErrorKind::IsADirectory)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// `open_raw` は、ディレクトリを読む向きでも作る向きでも、どの OS でも `NOT_REGULAR` で断る。種類は `Unsupported`
    /// で、IO の失敗の種類は `NotAvailable` になる (docs/spec/stdlib.md の「`Std.Stream`」)。
    #[test]
    fn a_raw_open_refuses_a_directory_as_not_a_regular_file() {
        let root = fixture("raw-dir");
        let host = StdHost::new(
            Box::new(io::sink()),
            Box::new(io::sink()),
            root.clone(),
            Buffering::Line,
        );
        for mode in [OpenMode::Read, OpenMode::Create, OpenMode::Append] {
            let e = host.open_raw(".", mode).err().unwrap();
            assert_eq!(e.kind(), io::ErrorKind::Unsupported, "{mode:?}");
            assert!(crate::is_not_regular(&e), "{mode:?}: {e}");
            assert_eq!(e.to_string(), crate::NOT_REGULAR);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// ないファイルを読む向きで開くと `NotFound` で、作る向きなら作って開く。
    #[test]
    fn a_raw_open_of_a_missing_file_is_not_found() {
        let root = fixture("raw-missing");
        let host = StdHost::new(
            Box::new(io::sink()),
            Box::new(io::sink()),
            root.clone(),
            Buffering::Line,
        );
        let e = host.open_raw("missing.txt", OpenMode::Read).err().unwrap();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        assert!(!crate::is_not_regular(&e));
        assert!(host.open_raw("made.txt", OpenMode::Create).is_ok());
        assert!(root.join("made.txt").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 文字列の標準入力の一時ファイルは、書いた中身を先頭から読ませる。unix では作ってすぐ消すので、一時ディレクトリに
    /// 名前が残らない。
    #[test]
    fn a_here_file_reads_back_its_bytes_and_has_no_name() {
        let bytes = "日本語\n".repeat(1000);
        let mut file = crate::here_file(bytes.as_bytes()).unwrap();
        let mut read = String::new();
        file.read_to_string(&mut read).unwrap();
        assert_eq!(read, bytes);
        #[cfg(unix)]
        {
            let prefix = format!("eml-stdin-{}-", std::process::id());
            let left = std::fs::read_dir(std::env::temp_dir())
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().starts_with(&prefix));
            assert!(!left);
        }
    }

    /// リダイレクトのファイルを開けなければ、子を起動せず、ファイルを使う段のプログラムとパスを誤りに出す。理由は
    /// 開けなかった誤りの種類の語である。
    #[test]
    fn a_launch_error_names_the_file_it_could_not_open() {
        use std::sync::Arc;

        use super::super::ctx::{fields, stage};
        use crate::{Fault, HostRef, IoReason, Launch, LaunchCause, launch};
        let root = fixture("launch");
        let host = StdHost::new(
            Box::new(io::sink()),
            Box::new(io::sink()),
            root.clone(),
            Buffering::Line,
        );
        let host = HostRef::new(Arc::new(host));
        let control = Control::new(Limits::default());
        let stages = stage("eml-no-such-program", &fields(&[]), &fields(&[]), "", 0);
        let failed = |stdin, payload, stdout, path| {
            launch(&host, &control, &stages, stdin, payload, stdout, path, None).err()
        };
        let launch_fault = |path: &str, reason| {
            Some(Fault::Launch(Box::new(Launch {
                program: "eml-no-such-program".into(),
                cause: LaunchCause::Open(path.into()),
                reason,
            })))
        };
        let missing = failed(4, "nope.txt", 0, "");
        assert_eq!(missing, launch_fault("nope.txt", IoReason::NotFound));
        assert_eq!(
            missing.unwrap().to_string(),
            "cannot run `eml-no-such-program`: cannot open `nope.txt`: not found"
        );
        assert_eq!(failed(0, "", 3, "."), launch_fault(".", IoReason::Other));
        assert_eq!(failed(4, ".", 0, ""), launch_fault(".", IoReason::Other));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 段を起動できなければ、それより前に起動した段を kill して待ってから、その段の `LaunchError` を返す。kill しな
    /// ければ、`sleep 30` を待つので戻らない。
    #[cfg(unix)]
    #[test]
    fn a_failed_stage_kills_the_stages_before_it() {
        use crate::{LaunchCause, LaunchError, Spawn, Stage, StderrSpec, StdinSpec, StdoutSpec};
        let (host, _, _) = std_host(Buffering::Line);
        let sleeper = || Stage {
            program: "sleep",
            args: &["30"],
            env: &[],
            cwd: None,
            stderr: StderrSpec::Null,
        };
        let missing = Stage {
            program: "eml-no-such-program",
            args: &[],
            ..sleeper()
        };
        let started = std::time::Instant::now();
        let error = host
            .spawn(Spawn {
                stages: vec![sleeper(), sleeper(), missing],
                stdin: StdinSpec::Null,
                stdout: StdoutSpec::Pipe,
            })
            .err()
            .unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        let inner = error
            .get_ref()
            .and_then(|e| e.downcast_ref::<LaunchError>());
        assert!(
            matches!(
                inner,
                Some(LaunchError {
                    stage: 2,
                    cause: LaunchCause::Spawn
                })
            ),
            "{inner:?}"
        );
    }

    /// 起動できなかった段の誤りは、その段のプログラムの名前の `Io { op: Run, .. }` になる。Host を呼ぶ前に断る誤りは、
    /// 最後の段の名前である。
    #[cfg(unix)]
    #[test]
    fn a_failed_stage_names_its_program() {
        use std::sync::Arc;

        use super::super::ctx::{fields, stage};
        use crate::{Fault, HostRef, IoOp, IoReason, launch};
        let (host, _, _) = std_host(Buffering::Line);
        let host = HostRef::new(Arc::new(host));
        let control = Control::new(Limits::default());
        let none = fields(&[]);
        let run = |stages: &[(&str, i64)]| {
            let stages: String = stages
                .iter()
                .map(|&(program, stderr)| stage(program, &none, &none, "", stderr))
                .collect();
            launch(&host, &control, &stages, 1, "", 1, "", None).err()
        };
        let not_found = |name: &str, reason| {
            Some(Fault::Io {
                op: IoOp::Run,
                name: name.into(),
                reason,
            })
        };
        assert_eq!(
            run(&[("true", 0), ("eml-missing-second", 0), ("true", 0)]),
            not_found("eml-missing-second", IoReason::NotFound)
        );
        assert_eq!(
            run(&[("eml-missing-first", 0), ("true", 0)]),
            not_found("eml-missing-first", IoReason::NotFound)
        );
        assert_eq!(
            run(&[("eml-first", 0), ("eml-last", 9)]),
            not_found("eml-last", IoReason::Other)
        );
    }

    /// 標準エラーの pipe (3) は、`pipes` を渡した最後の段だけが使える。`pipes` は標準入力、標準出力、標準エラーの順に
    /// 呼ばれ、標準エラーの子の端が子の標準エラーになる。ほかの段の 3 と、`pipes` のない 3 は、`pipes` を呼ぶ前に
    /// 起動の誤りにする。
    #[cfg(unix)]
    #[test]
    fn a_stderr_pipe_is_given_to_the_last_stage_only() {
        use std::sync::Arc;

        use super::super::ctx::{fields, stage};
        use crate::{Fault, HostRef, IoOp, IoReason, OsHandle, PipeEnd, launch};
        let (host, _, _) = std_host(Buffering::Line);
        let host = HostRef::new(Arc::new(host));
        let control = Control::new(Limits::default());
        let none = fields(&[]);
        let talk = fields(&["-c", "echo out; echo err >&2"]);
        let mut ends = Vec::new();
        let mut readers = Vec::new();
        let mut pipes = |end: PipeEnd| -> io::Result<OsHandle> {
            ends.push(end);
            let (reader, writer) = io::pipe()?;
            // 書く端を閉じるので、子の標準入力はすぐに終わる
            if end == PipeEnd::Stdin {
                return Ok(OsHandle::from(reader));
            }
            readers.push(reader);
            Ok(OsHandle::from(writer))
        };
        let invalid = Some(Fault::Io {
            op: IoOp::Run,
            name: "sh".into(),
            reason: IoReason::Other,
        });
        let first = stage("sh", &talk, &none, "", 3) + &stage("sh", &talk, &none, "", 0);
        let error = launch(&host, &control, &first, 1, "", 1, "", Some(&mut pipes)).err();
        assert_eq!(error, invalid);
        let last = stage("sh", &talk, &none, "", 3);
        assert_eq!(
            launch(&host, &control, &last, 1, "", 1, "", None).err(),
            invalid
        );
        let launched = launch(&host, &control, &last, 2, "", 2, "", Some(&mut pipes));
        let mut processes = launched.unwrap().spawned.processes;
        assert_eq!(processes[0].wait().unwrap(), 0);
        assert_eq!(ends, [PipeEnd::Stdin, PipeEnd::Stdout, PipeEnd::Stderr]);
        let read = |reader: &mut io::PipeReader| {
            let mut text = String::new();
            reader.read_to_string(&mut text).unwrap();
            text
        };
        assert_eq!(read(&mut readers[0]), "out\n");
        assert_eq!(read(&mut readers[1]), "err\n");
    }
}
