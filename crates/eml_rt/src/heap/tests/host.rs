//! テストの Host。出力をメモリに受け、ファイルをメモリから読ませる。Miri でも動くように、OS に触れない。

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::{Host, HostRef, Opened, Stream};

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
    /// 読むたびにこの `Control` に割り込みを立て、`Interrupted` を返す。SIGINT のハンドラが止める語を立てた後に、
    /// 待っている読みが `EINTR` で戻ることを表す。100 回を超えると、読み直しが続いたことを `Interrupted` でない誤りで
    /// 知らせる
    pub(super) interrupt_with: Option<Arc<crate::Control>>,
    /// 読み手が落ちた回数
    pub(super) drops: Option<Arc<AtomicUsize>>,
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
    interrupts: usize,
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(control) = &self.file.interrupt_with {
            self.interrupts += 1;
            if self.interrupts > 100 {
                return Err(io::Error::other("the read kept being interrupted"));
            }
            control.interrupt();
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        if self.file.interrupt_once && !self.interrupted {
            self.interrupted = true;
            return Err(io::Error::from(io::ErrorKind::Interrupted));
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
}

impl MemoryHost {
    pub(super) fn failing(message: &'static str) -> MemoryHost {
        MemoryHost {
            fail_write: Some(message),
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
    fn flush(&self) -> io::Result<()> {
        Ok(())
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
        let reader: Box<dyn BufRead + Send> = Box::new(BufReader::new(Reader {
            file,
            at: 0,
            interrupted: false,
            interrupts: 0,
        }));
        Ok(Opened { reader, size })
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
    use std::io::{self, Read};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::path::PathBuf;

    use super::Shared;
    use crate::{Buffering, Host, StdHost, Stream};

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
