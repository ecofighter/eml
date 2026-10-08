use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;

/// インタプリタはシングルスレッドで動くので、書き込み先を `Rc` と `RefCell` で共有する。出力先は値でもフレームでもないので、
/// docs/spec/core-ir.md の「実行時の規約」の `Rc` と `RefCell` を使わない規則の外にある。
#[derive(Clone)]
pub struct OutputSink(Rc<RefCell<dyn Write>>);

impl OutputSink {
    pub fn new(writer: impl Write + 'static) -> Self {
        OutputSink(Rc::new(RefCell::new(writer)))
    }

    pub fn stdout() -> Self {
        Self::new(io::stdout())
    }

    /// テストでプログラムの出力を捕まえるためのもの。
    pub fn capture() -> (Self, Captured) {
        let buffer = Rc::new(RefCell::new(Vec::new()));
        (OutputSink(buffer.clone()), Captured(buffer))
    }

    pub fn write_str(&self, text: &str) -> io::Result<()> {
        let mut writer = self.0.borrow_mut();
        writer.write_all(text.as_bytes())?;
        writer.flush()
    }

    /// `println` のため。中身を写して改行を付けた文字列を作らずに書き、flush は1回だけにする。
    pub fn write_line(&self, text: &str) -> io::Result<()> {
        let mut writer = self.0.borrow_mut();
        writer.write_all(text.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()
    }
}

/// `OutputSink::capture` が捕まえた出力。
#[derive(Clone)]
pub struct Captured(Rc<RefCell<Vec<u8>>>);

impl Captured {
    /// プログラムは文字列だけを書くので、出力は UTF-8 である。
    pub fn contents(&self) -> String {
        String::from_utf8(self.0.borrow().clone()).expect("the program writes UTF-8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Rc` を持つので `Send` でない書き込み先。
    struct Local(Rc<RefCell<Vec<u8>>>);

    impl Write for Local {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_sink_accepts_a_writer_that_is_not_send() {
        let buffer = Rc::new(RefCell::new(Vec::new()));
        let sink = OutputSink::new(Local(buffer.clone()));
        sink.write_str("hello\n").unwrap();
        assert_eq!(*buffer.borrow(), b"hello\n");
    }

    #[test]
    fn capture_collects_writes_from_clones() {
        let (sink, captured) = OutputSink::capture();
        let clone = sink.clone();
        sink.write_str("hello ").unwrap();
        clone.write_str("world\n").unwrap();
        assert_eq!(captured.contents(), "hello world\n");
    }

    #[test]
    fn write_line_ends_the_text_with_a_newline() {
        let (sink, captured) = OutputSink::capture();
        sink.write_line("hello").unwrap();
        assert_eq!(captured.contents(), "hello\n");
    }
}
