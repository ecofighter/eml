//! driver が起こすスレッドのシグナルの塞ぎ。unix で SIGINT と SIGTERM だけを塞ぎ、シグナルを受けるのを VM のスレッドに
//! 限る (docs/spec/vm.md の「上限と割り込み」)。新しいスレッドは起こしたスレッドの塞ぎを受け継ぐので、起こす間だけ
//! 呼んだスレッドで塞ぐ。SIGSEGV などの同期のシグナルを塞ぐと動きが未定義になるので、塞ぐのはこの2つに限る。

/// 塞いでいる間だけ生きる。落とすと、塞ぐ前の集合に戻す。
#[cfg(unix)]
pub(crate) struct Masked {
    old: libc::sigset_t,
}

#[cfg(unix)]
impl Masked {
    pub(crate) fn new() -> Masked {
        // SAFETY: 2つの集合はこの関数の局所変数で、0 で埋めた `sigset_t` は `sigemptyset` の前の値として正しい。
        // `pthread_sigmask` は渡した集合を読み書きするだけである。正しい引数ではどの呼び出しも失敗しない
        unsafe {
            let mut block: libc::sigset_t = std::mem::zeroed();
            let mut old: libc::sigset_t = std::mem::zeroed();
            let mut status = libc::sigemptyset(&mut block);
            status |= libc::sigaddset(&mut block, libc::SIGINT);
            status |= libc::sigaddset(&mut block, libc::SIGTERM);
            status |= libc::pthread_sigmask(libc::SIG_BLOCK, &block, &mut old);
            debug_assert_eq!(status, 0);
            Masked { old }
        }
    }
}

#[cfg(unix)]
impl Drop for Masked {
    fn drop(&mut self) {
        // SAFETY: `old` は `new` が読んだ集合で、`pthread_sigmask` は読むだけである
        let status =
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.old, std::ptr::null_mut()) };
        debug_assert_eq!(status, 0);
    }
}

/// Windows のコンソールの handler は OS が作るスレッドで動くので、塞ぐものがない。
#[cfg(not(unix))]
pub(crate) struct Masked;

#[cfg(not(unix))]
impl Masked {
    pub(crate) fn new() -> Masked {
        Masked
    }
}

/// SIGINT と SIGTERM を塞いだまま、スレッドを始める。新しいスレッドは始めたスレッドの塞ぎを受け継ぐので、`spawn` の
/// 間だけ呼んだスレッドで塞ぐ。監視のスレッドと名前の解決のスレッドがこれで始まり、2つのシグナルは VM のスレッドに
/// 届く (docs/spec/vm.md の「上限と割り込み」)。unix でない OS では、ふつうの `spawn` と同じである。
pub fn spawn_masked<F, T>(
    builder: std::thread::Builder,
    body: F,
) -> std::io::Result<std::thread::JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let _masked = Masked::new();
    builder.spawn(body)
}
