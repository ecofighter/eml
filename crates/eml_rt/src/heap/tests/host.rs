//! テストの Host。出力をメモリに受け、ファイルをメモリから読ませる。Miri でも動くように、OS に触れない。

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, Cursor, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::{
    Host, HostRef, Opened, Process, Spawn, Spawned, StderrMode, StdinSlot, Stdio, Stream, WriteMode,
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
    pub(super) stdio: (Stdio, Stdio, StderrMode),
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
    /// プロセスを起動せず、受けた設定を覚える。`missing` は見つからないプログラムである。pipe の標準入力は
    /// `child_input` に写し (`fail_write` があれば書くたびにその文言で失敗する)、pipe の標準出力は `child_output` を
    /// 読ませる。子の `wait` は引数の数を終了コードにする
    fn spawn(&self, spawn: &Spawn<'_>) -> io::Result<Spawned> {
        if spawn.program == "missing" {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        self.spawns.lock().unwrap().push(Started {
            program: spawn.program.to_string(),
            args: spawn.args.iter().map(|a| a.to_string()).collect(),
            env: spawn
                .env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            cwd: spawn.cwd.map(str::to_string),
            stdio: (spawn.stdin, spawn.stdout, spawn.stderr),
        });
        let stdin = (spawn.stdin == Stdio::Pipe).then(|| {
            Box::new(ChildInput {
                bytes: self.child_input.clone(),
                fail: self.fail_write,
                interrupt_once: self.child_interrupts_once,
                interrupt: self.interrupt_run.clone(),
            }) as Box<dyn Write + Send>
        });
        let stdout = (spawn.stdout == Stdio::Pipe)
            .then(|| Box::new(Cursor::new(self.child_output.clone())) as Box<dyn Read + Send>);
        Ok(Spawned {
            stdin,
            stdout,
            process: Box::new(MemoryProcess {
                code: spawn.args.len() as i32,
                waits: self.waits.clone(),
                kills: self.kills.clone(),
                interrupt: self.interrupt_run.clone(),
            }),
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
        // 旗のロックを放してから panic する。持ったままだと、旗のロックまで毒されて次の書き込みが止まる
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
    use crate::{Buffering, Control, Host, Limits, StdHost, Stream, WriteMode};

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
        use crate::{Spawn, StderrMode, Stdio};
        let (host, out, _) = std_host(Buffering::Block);
        host.write_line(Stream::Out, "before").unwrap();
        let spawn = Spawn {
            program: "sh",
            args: &["-c", "cat; echo err >&2; exit 3"],
            env: &[],
            cwd: None,
            stdin: Stdio::Pipe,
            stdout: Stdio::Pipe,
            stderr: StderrMode::ToStdout,
        };
        let mut child = host.spawn(&spawn).unwrap();
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
        assert_eq!(child.process.wait().unwrap(), 3);
        let sleeper = Spawn {
            args: &["-c", "sleep 10"],
            stdin: Stdio::Null,
            stdout: Stdio::Null,
            stderr: StderrMode::Null,
            ..spawn
        };
        let mut child = host.spawn(&sleeper).unwrap();
        assert!(child.stdin.is_none() && child.stdout.is_none());
        child.process.kill().unwrap();
        assert_eq!(child.process.wait().unwrap(), 128 + 9);
        let missing = Spawn {
            program: "eml-no-such-program",
            ..sleeper
        };
        assert_eq!(
            host.spawn(&missing).err().map(|e| e.kind()),
            Some(io::ErrorKind::NotFound)
        );
    }

    /// 作業ディレクトリは `file_root` につなぎ、`/` を含む相対のプログラムはそこに対して解決する。環境変数は上書き
    /// する。
    #[cfg(unix)]
    #[test]
    fn a_spawned_child_runs_in_its_directory() {
        use std::os::unix::fs::PermissionsExt;

        use crate::{Spawn, StderrMode, Stdio};
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
            program: "./run.sh",
            args: &[],
            env: &[("FOO", "a"), ("FOO", "b")],
            cwd: Some("sub"),
            stdin: Stdio::Null,
            stdout: Stdio::Pipe,
            stderr: StderrMode::Inherit,
        };
        let mut child = host.spawn(&spawn).unwrap();
        let mut read = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut read)
            .unwrap();
        assert_eq!(read, "b hi\n");
        assert_eq!(child.process.wait().unwrap(), 0);
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

    /// 書き込み先が panic してロックが毒されても、次の行は書ける。panic した行は `BufWriter` に残り、次の行と一緒に
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

    /// ディレクトリも開ける (読むときに誤りになる)。長さの見込みは返さない。ディレクトリを開けるのは
    /// Unix の振る舞いである。
    #[cfg(unix)]
    #[test]
    fn a_directory_opens_without_a_size() {
        let root = fixture("dir");
        let host = StdHost::new(
            Box::new(io::sink()),
            Box::new(io::sink()),
            root.clone(),
            Buffering::Line,
        );
        assert_eq!(host.open(".").unwrap().size, None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
