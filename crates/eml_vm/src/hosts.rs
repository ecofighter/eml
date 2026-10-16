//! VM を動かす Host (docs/spec/vm.md の「置き場所」)。`eml run` と REPL の Host と、テストが出力を読み、入力を与える
//! Host を置く。

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Cursor, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};

use eml_rt::{Buffering, Host, HostRef, Opened, StdHost, StdinLock, Stream, exit_code, lock_stdin};

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

/// `CaptureHost` が書いたファイル。パスの順に並ぶ。
#[derive(Clone, Default)]
pub struct Written(Arc<Mutex<BTreeMap<String, String>>>);

impl Written {
    pub fn files(&self) -> Vec<(String, String)> {
        let files = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        files
            .iter()
            .map(|(path, text)| (path.clone(), text.clone()))
            .collect()
    }

    fn get(&self, path: &str) -> Option<String> {
        let files = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        files.get(path).cloned()
    }
}

/// テストの Host。標準出力をメモリに捕まえ、標準エラーは捨てる。引数、標準入力、環境変数は与えたものだけで、本物の
/// 環境は読まない。テストの結果を実行する場所によらないものにするためである。`Fs.write_text` はディスクに書かずに
/// 覚え、同じパスの `Fs.open` はそれを読む。リポジトリの中にファイルを作らないためである。子のプロセスは本物を起動し、
/// その標準出力も捕まえる (docs/implementation/testing.md の「UI テスト」)。
pub struct CaptureHost {
    // 出力とファイルの読みは `eml run` と同じ `StdHost` に任せ、出力のまとめ方も同じにする
    inner: StdHost,
    out: Capture,
    written: Written,
    file_root: PathBuf,
    args: Vec<OsString>,
    env: Vec<(String, OsString)>,
    /// `None` なら標準入力を使えない (REPL)。
    stdin: Option<Mutex<Box<dyn BufRead + Send>>>,
}

impl CaptureHost {
    /// 引数なし、空の標準入力、空の環境で始める。`file_root` は `Fs.open` の相対パスと子の作業ディレクトリの基準である。
    pub fn new(file_root: PathBuf) -> CaptureHost {
        let out = Capture::default();
        let inner = StdHost::new(
            Box::new(out.clone()),
            Box::new(io::sink()),
            file_root.clone(),
            Buffering::Block,
        );
        CaptureHost {
            inner,
            out,
            written: Written::default(),
            file_root,
            args: Vec::new(),
            env: Vec::new(),
            stdin: None,
        }
        .with_stdin(Vec::new())
    }

    pub fn with_args(mut self, args: Vec<OsString>) -> CaptureHost {
        self.args = args;
        self
    }

    pub fn with_stdin(mut self, bytes: Vec<u8>) -> CaptureHost {
        let reader: Box<dyn BufRead + Send> = Box::new(BufReader::new(Cursor::new(bytes)));
        self.stdin = Some(Mutex::new(reader));
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

    pub fn output(&self) -> Capture {
        self.out.clone()
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

    fn flush(&self) -> io::Result<()> {
        self.inner.flush()
    }

    fn open(&self, path: &str) -> io::Result<Opened> {
        match self.written.get(path) {
            Some(text) => {
                let size = text.len() as u64;
                Ok(Opened {
                    reader: Box::new(Cursor::new(text.into_bytes())),
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

    fn stdin(&self) -> io::Result<StdinLock<'_>> {
        match &self.stdin {
            Some(stdin) => Ok(lock_stdin(stdin)),
            None => Err(io::Error::from(io::ErrorKind::Unsupported)),
        }
    }

    fn write_file(&self, path: &str, text: &str) -> io::Result<()> {
        let mut files = self
            .written
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        files.insert(path.to_string(), text.to_string());
        Ok(())
    }

    /// 子の環境は与えた表に本物の `PATH` を足したもので、標準入力は空である。子の標準出力は、それまでの出力を書き出した
    /// 後で、捕まえた出力に足す。
    fn run(&self, program: &str, args: &[&str]) -> io::Result<i32> {
        self.inner.flush()?;
        let mut command = Command::new(program);
        command
            .args(args)
            .env_clear()
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        if !self.file_root.as_os_str().is_empty() {
            command.current_dir(&self.file_root);
        }
        let output = command.output()?;
        self.out.clone().write_all(&output.stdout)?;
        Ok(exit_code(output.status))
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
        .with_stdin(Box::new(BufReader::new(io::stdin())));
    HostRef::new(Arc::new(host))
}
