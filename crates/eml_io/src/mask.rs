//! VMのスレッド (`eml run`ではworker 0) のほかのすべてのスレッドでSIGINTとSIGTERMを塞ぎ、2つのシグナルをそのスレッド
//! だけに届ける (docs/spec/vm.mdの「上限と割り込み」)。新しいスレッドは自分の本体の始めに2つを塞いで外さないので、
//! それを作ったスレッドは、worker 0も含め、塞がない。SIGSEGVなどの同期のシグナルを塞ぐと動きが未定義になるので、塞ぐのは
//! この2つに限る。

#![allow(unsafe_code)]

#[cfg(all(unix, not(miri)))]
const SIGNALS: [libc::c_int; 2] = [libc::SIGINT, libc::SIGTERM];

/// 生きている間2つのシグナルを塞ぐ。落とすと、塞ぐ前の集合に戻す。`eml_vm::execute`を呼んだスレッドは、worker 0が
/// 最初の命令を動かす前からworker 0をjoinし終えるまで、これで塞ぐ (docs/spec/vm.mdの「上限と割り込み」)。
#[cfg(all(unix, not(miri)))]
pub struct Masked {
    old: libc::sigset_t,
}

// 作ると今のスレッドを塞ぐので、`Default`で黙って作れる形にしない
#[cfg(all(unix, not(miri)))]
#[expect(clippy::new_without_default, reason = "creating one blocks signals")]
impl Masked {
    #[must_use]
    pub fn new() -> Masked {
        Masked {
            old: block(&SIGNALS),
        }
    }
}

#[cfg(all(unix, not(miri)))]
impl Drop for Masked {
    fn drop(&mut self) {
        // SAFETY: `old`は`new`が読んだ集合で、`pthread_sigmask`は読むだけである
        let status =
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.old, std::ptr::null_mut()) };
        debug_assert_eq!(status, 0);
    }
}

/// WindowsのコンソールのhandlerはOSが作るスレッドで動くので、塞ぐものがない。Miriは`pthread_sigmask`を
/// 持たず、シグナルも届かないので、Miriでも塞がない。
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

/// 生きている間、このスレッドで2つのシグナルの塞ぎを外す。落とすと塞ぐ前の集合に戻し、Appleでは自分のスレッドの
/// 保留に残ったものを送り直す。子のプロセスは始めたスレッドの塞ぎを受け継ぐので、2つを塞ぐスレッドは子を始める間
/// これで塞ぎを外す。外さなければ、Ctrl-CとSIGTERMでその子を終わらせられない (docs/spec/stdlib.mdの「`Proc`」)。
/// 2つを塞がないworker 0では何も変わらない。
#[cfg(all(unix, not(miri)))]
pub struct Unmasked {
    old: libc::sigset_t,
}

#[cfg(all(unix, not(miri)))]
#[expect(clippy::new_without_default, reason = "creating one unblocks signals")]
impl Unmasked {
    #[must_use]
    pub fn new() -> Unmasked {
        let set = set_of(&SIGNALS);
        // SAFETY: 2つの集合はこの関数の局所変数で、`pthread_sigmask`は渡した集合を読み書きするだけである
        unsafe {
            let mut old: libc::sigset_t = std::mem::zeroed();
            let status = libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, &mut old);
            debug_assert_eq!(status, 0);
            Unmasked { old }
        }
    }
}

#[cfg(all(unix, not(miri)))]
impl Drop for Unmasked {
    fn drop(&mut self) {
        // SAFETY: `old`は`new`が読んだ集合で、`pthread_sigmask`は読むだけである
        let status =
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.old, std::ptr::null_mut()) };
        debug_assert_eq!(status, 0);
        repost(&SIGNALS);
    }
}

/// `Masked`と同じく、unixでないOSとMiriでは何もしない。
#[cfg(any(not(unix), miri))]
pub struct Unmasked;

#[cfg(any(not(unix), miri))]
#[expect(
    clippy::new_without_default,
    reason = "creating one unblocks signals on unix"
)]
impl Unmasked {
    #[must_use]
    pub fn new() -> Unmasked {
        Unmasked
    }
}

/// `signals`の集合。
#[cfg(all(unix, not(miri)))]
fn set_of(signals: &[libc::c_int]) -> libc::sigset_t {
    // SAFETY: 集合はこの関数の局所変数で、0で埋めた`sigset_t`は`sigemptyset`の前の値として正しい。正しい
    // シグナルの番号ではどの呼び出しも失敗しない
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        let mut status = libc::sigemptyset(&mut set);
        for &signal in signals {
            status |= libc::sigaddset(&mut set, signal);
        }
        debug_assert_eq!(status, 0);
        set
    }
}

/// このスレッドで`signals`を塞ぎ、塞ぐ前の集合を返す。
#[cfg(all(unix, not(miri)))]
fn block(signals: &[libc::c_int]) -> libc::sigset_t {
    let set = set_of(signals);
    // SAFETY: 2つの集合はこの関数の局所変数で、`pthread_sigmask`は渡した集合を読み書きするだけである
    let old = unsafe {
        let mut old: libc::sigset_t = std::mem::zeroed();
        let status = libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut old);
        debug_assert_eq!(status, 0);
        old
    };
    repost(signals);
    old
}

/// Appleでは、すべてのスレッドが塞いでいる間に来たシグナルは1つのスレッドの保留に残り、後で塞ぎを外したほかの
/// スレッドもそれを受けない。そこで、`signals`のうちこのスレッドが塞いでいて保留になっているものを`sigwait`で取り、
/// プロセスへ送り直す。塞いでいないスレッドがそれを受ける。Linuxでは、そうしたシグナルはプロセス全体の集合に残り、
/// 最初に塞ぎを外したスレッドが受ける。Linuxの`sigpending`はその集合も返し、ほかのスレッドが先に受けると`sigwait`が
/// 待ち続けうるので、何もしない。
#[cfg(all(unix, not(miri)))]
fn repost(signals: &[libc::c_int]) {
    #[cfg(target_vendor = "apple")]
    // SAFETY: 集合はこの関数の局所変数で、`sigpending`と`pthread_sigmask`は書くだけ、`sigismember`は読むだけで
    // ある。`sigwait`は、このスレッドが塞いでいて保留になっているシグナルだけを待つので、すぐに戻る
    unsafe {
        let mut pending: libc::sigset_t = std::mem::zeroed();
        let mut mask: libc::sigset_t = std::mem::zeroed();
        if libc::sigpending(&mut pending) != 0
            || libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), &mut mask) != 0
        {
            return;
        }
        for &signal in signals {
            if libc::sigismember(&pending, signal) == 1 && libc::sigismember(&mask, signal) == 1 {
                let set = set_of(&[signal]);
                let mut taken = 0;
                if libc::sigwait(&set, &mut taken) == 0 {
                    libc::kill(libc::getpid(), taken);
                }
            }
        }
    }
    #[cfg(not(target_vendor = "apple"))]
    let _ = signals;
}

/// 今のスレッドでSIGINTとSIGTERMを塞ぎ、戻さない。unixでないOSとMiriでは何もしない。
fn block_for_good() {
    #[cfg(all(unix, not(miri)))]
    block(&SIGNALS);
}

/// `body`を動かす前にSIGINTとSIGTERMを塞ぐスレッドを始める。呼んだスレッドの塞ぎには触れないので、worker 0は
/// スレッドを始める間も2つのシグナルを受けられる。監視のスレッド、driverのプールと名前の解決のスレッド、
/// `CaptureHost`の読むスレッドがこれで始まる (docs/spec/vm.mdの「上限と割り込み」)。unixでないOSでは、ふつうの
/// `spawn`と同じである。
pub fn spawn_masked<F, T>(
    builder: std::thread::Builder,
    body: F,
) -> std::io::Result<std::thread::JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    builder.spawn(move || {
        block_for_good();
        body()
    })
}

/// `spawn_masked`のscoped版。`thread::scope`の中で、借りた値を使うスレッドを塞いで始める。
pub fn spawn_scoped_masked<'scope, 'env, F, T>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    builder: std::thread::Builder,
    body: F,
) -> std::io::Result<std::thread::ScopedJoinHandle<'scope, T>>
where
    F: FnOnce() -> T + Send + 'scope,
    T: Send + 'scope,
{
    builder.spawn_scoped(scope, move || {
        block_for_good();
        body()
    })
}

#[cfg(all(test, target_vendor = "apple", not(miri)))]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use super::*;

    static HANDLED: AtomicUsize = AtomicUsize::new(0);
    static HANDLED_ON: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn count(_: libc::c_int) {
        // SAFETY: `pthread_self`は引数を持たず、失敗しない
        HANDLED_ON.store(unsafe { libc::pthread_self() } as usize, Ordering::SeqCst);
        HANDLED.fetch_add(1, Ordering::SeqCst);
    }

    fn pending_here(signal: libc::c_int) -> bool {
        // SAFETY: 集合はこの関数の局所変数で、`sigpending`は書くだけである
        unsafe {
            let mut pending: libc::sigset_t = std::mem::zeroed();
            assert_eq!(libc::sigpending(&mut pending), 0);
            libc::sigismember(&pending, signal) == 1
        }
    }

    fn mask(how: libc::c_int, signal: libc::c_int) {
        let set = set_of(&[signal]);
        // SAFETY: 集合はこの関数の局所変数で、`pthread_sigmask`は読むだけである
        let status = unsafe { libc::pthread_sigmask(how, &set, std::ptr::null_mut()) };
        assert_eq!(status, 0);
    }

    /// 塞いだスレッドの保留に残ったシグナルは、送り直すと、塞いでいないスレッドに届く。
    #[test]
    fn a_pinned_signal_is_reposted_to_another_thread() {
        // SAFETY: `count`はアトミックな書き込みだけをし、`action`はこの関数の局所変数である
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = count as *const () as libc::sighandler_t;
            libc::sigemptyset(&mut action.sa_mask);
            assert_eq!(
                libc::sigaction(libc::SIGUSR2, &action, std::ptr::null_mut()),
                0
            );
        }
        // 塞ぐ前に作るので、このスレッドは塞がずに、ハンドラが動くまで待つ。送り直したシグナルは塞いでいないスレッドの
        // どれかが受ける。macOSでは、いちばん古いテストのハーネスのメインスレッドが受ける
        let waiter = std::thread::spawn(|| {
            let deadline = Instant::now() + Duration::from_secs(1);
            while HANDLED.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        mask(libc::SIG_BLOCK, libc::SIGUSR2);
        // SAFETY: 自分のスレッドへ送るので、相手のスレッドは生きている
        assert_eq!(
            unsafe { libc::pthread_kill(libc::pthread_self(), libc::SIGUSR2) },
            0
        );
        assert!(pending_here(libc::SIGUSR2));
        repost(&[libc::SIGUSR2]);
        assert!(!pending_here(libc::SIGUSR2));
        waiter.join().unwrap();
        mask(libc::SIG_UNBLOCK, libc::SIGUSR2);
        assert_eq!(HANDLED.load(Ordering::SeqCst), 1);
        // SAFETY: `pthread_self`は引数を持たず、失敗しない
        let here = unsafe { libc::pthread_self() } as usize;
        assert_ne!(HANDLED_ON.load(Ordering::SeqCst), here);
    }
}
