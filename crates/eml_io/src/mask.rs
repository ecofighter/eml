//! worker 0 のほかのスレッドのシグナルの塞ぎ。unix で SIGINT と SIGTERM だけを塞ぎ、シグナルを受けるのを worker 0 の
//! スレッドに限る (docs/spec/vm.md の「上限と割り込み」)。新しいスレッドは起こしたスレッドの塞ぎを受け継ぐので、起こす間だけ
//! 呼んだスレッドで塞ぐ。SIGSEGV などの同期のシグナルを塞ぐと動きが未定義になるので、塞ぐのはこの2つに限る。

#![allow(unsafe_code)]

/// 塞いでいる間だけ生きる。落とすと、塞ぐ前の集合に戻す。`eml_vm::execute` の host のスレッドも、worker 0 を join
/// する間これで塞ぐ (docs/spec/vm.md の「上限と割り込み」)。
#[cfg(all(unix, not(miri)))]
pub struct Masked {
    old: libc::sigset_t,
}

// 作ると今のスレッドを塞ぐので、`Default` で黙って作れる形にしない
#[cfg(all(unix, not(miri)))]
#[expect(clippy::new_without_default, reason = "creating one blocks signals")]
impl Masked {
    #[must_use]
    pub fn new() -> Masked {
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

#[cfg(all(unix, not(miri)))]
impl Drop for Masked {
    fn drop(&mut self) {
        // SAFETY: `old` は `new` が読んだ集合で、`pthread_sigmask` は読むだけである
        let status =
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.old, std::ptr::null_mut()) };
        debug_assert_eq!(status, 0);
    }
}

/// Windows のコンソールの handler は OS が作るスレッドで動くので、塞ぐものがない。Miri は `pthread_sigmask` を
/// 持たず、シグナルも届かないので、Miri でも塞がない。
#[cfg(any(not(unix), miri))]
pub struct Masked;

#[cfg(any(not(unix), miri))]
#[expect(
    clippy::new_without_default,
    reason = "creating one blocks signals on unix"
)]
impl Masked {
    #[must_use]
    pub fn new() -> Masked {
        Masked
    }
}

/// SIGINT と SIGTERM を塞いだまま、スレッドを始める。新しいスレッドは始めたスレッドの塞ぎを受け継ぐので、`spawn` の
/// 間だけ呼んだスレッドで塞ぐ。監視のスレッド、driver のプールと名前の解決のスレッド、`CaptureHost` の読むスレッドが
/// これで始まり、2つのシグナルは worker 0 のスレッドに届く (docs/spec/vm.md の「上限と割り込み」)。unix でない OS では、ふつうの `spawn` と同じである。
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

/// `spawn_masked` の scoped 版。`thread::scope` の中で、借りた値を使うスレッドを塞いで始める。
pub fn spawn_scoped_masked<'scope, 'env, F, T>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    builder: std::thread::Builder,
    body: F,
) -> std::io::Result<std::thread::ScopedJoinHandle<'scope, T>>
where
    F: FnOnce() -> T + Send + 'scope,
    T: Send + 'scope,
{
    let _masked = Masked::new();
    builder.spawn_scoped(scope, body)
}
