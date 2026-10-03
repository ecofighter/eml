use std::io::{self, Write};
use std::sync::{Arc, Mutex};

/// 将来、複数のスレッドから `println` するため `Send + Sync` にする (docs/future/multicore.md)。
#[derive(Clone)]
pub struct OutputSink(Arc<Mutex<dyn Write + Send>>);

impl OutputSink {
    pub fn new(writer: impl Write + Send + 'static) -> Self {
        OutputSink(Arc::new(Mutex::new(writer)))
    }

    pub fn stdout() -> Self {
        Self::new(io::stdout())
    }

    /// テストでプログラムの出力を捕まえるためのもの。
    pub fn capture() -> (Self, Arc<Mutex<Vec<u8>>>) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        (OutputSink(buffer.clone()), buffer)
    }

    pub fn write_str(&self, text: &str) -> io::Result<()> {
        let mut writer = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        writer.write_all(text.as_bytes())?;
        writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn output_sink_is_send_and_sync() {
        assert_send_sync::<OutputSink>();
    }

    #[test]
    fn capture_collects_writes_from_clones() {
        let (sink, buffer) = OutputSink::capture();
        let clone = sink.clone();
        sink.write_str("hello ").unwrap();
        clone.write_str("world\n").unwrap();
        assert_eq!(
            String::from_utf8(buffer.lock().unwrap().clone()).unwrap(),
            "hello world\n"
        );
    }
}
