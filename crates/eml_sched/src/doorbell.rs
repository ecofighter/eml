//! 自己パイプ (`Doorbell`)。シグナルのハンドラが鳴らし、監視のスレッドが待つ (docs/spec/vm.md の「タスクとスケジュール」)。
//! `Machine` ごとに1つで、プロセスに1つの static にはしない。1つのプロセスで動く `cargo test` の中で、監視のスレッドどうしが
//! 起こしを取り合わないためである。

use std::io;
use std::sync::Arc;
use std::time::Duration;

/// 監視のスレッドを起こす呼び鈴。unix では非同期のシグナルのハンドラから鳴らせる自己パイプで、Windows ではコンソールの
/// handler が普通のスレッドで動くので、旗と condvar である。
pub struct Doorbell {
    #[cfg(unix)]
    read: libc::c_int,
    #[cfg(unix)]
    write: libc::c_int,
    #[cfg(not(unix))]
    rung: std::sync::Mutex<bool>,
    #[cfg(not(unix))]
    bell: std::sync::Condvar,
}

#[cfg(unix)]
impl Doorbell {
    /// 両端を非同期にして、`exec` した子に渡さない。鳴らす側は満ちたパイプで待たず、待つ側は空になるまで読み切る。
    pub fn new() -> io::Result<Arc<Doorbell>> {
        let mut fds = [0; 2];
        // SAFETY: `pipe` は渡した2つの int に書くだけである
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // 先に作っておけば、途中で失敗しても `Drop` が両端を閉じる。macOS には `pipe2` がないので `fcntl` で付ける
        let doorbell = Doorbell {
            read: fds[0],
            write: fds[1],
        };
        for fd in fds {
            set_flag(fd, libc::F_GETFL, libc::F_SETFL, libc::O_NONBLOCK)?;
            set_flag(fd, libc::F_GETFD, libc::F_SETFD, libc::FD_CLOEXEC)?;
        }
        Ok(Arc::new(doorbell))
    }

    /// 1バイト書く。async-signal-safe な `write` だけを呼び、ハンドラが割り込んだ処理の `errno` を戻す。パイプが満ちて
    /// いれば (`EAGAIN`) もう鳴っているので、書けなくても構わない。
    pub fn ring(&self) {
        let saved = errno::get();
        let byte = 1u8;
        // SAFETY: `byte` は呼び出しの間生きている1バイトで、`write` の fd は `Drop` まで閉じない
        let _ = unsafe { libc::write(self.write, (&raw const byte).cast(), 1) };
        errno::set(saved);
    }

    /// 鳴るか `timeout` が過ぎるまで待ち、たまったバイトを読み切る。早く戻ってもよく、呼ぶ側は戻るたびに確かめ直す。
    /// 何度鳴っても1回の起床にまとまる。鳴っていたかを返す。
    pub fn wait(&self, timeout: Option<Duration>) -> bool {
        let ms = timeout.map_or(-1, |t| {
            // 切り上げる。切り捨てると、1 ms に満たない残りで眠らずに回り続ける
            libc::c_int::try_from(t.as_nanos().div_ceil(1_000_000)).unwrap_or(libc::c_int::MAX)
        });
        let mut fd = libc::pollfd {
            fd: self.read,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `fd` は1つの `pollfd` で、`poll` はその `revents` に書くだけである。`EINTR` でも早く戻るだけである
        unsafe { libc::poll(&mut fd, 1, ms) };
        let mut buf = [0u8; 64];
        let mut rung = false;
        loop {
            // SAFETY: `buf` はこの関数の局所変数で、`read` はその長さまで書く
            let n = unsafe { libc::read(self.read, buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                return rung;
            }
            rung = true;
        }
    }
}

#[cfg(unix)]
fn set_flag(
    fd: libc::c_int,
    get: libc::c_int,
    set: libc::c_int,
    flag: libc::c_int,
) -> io::Result<()> {
    // SAFETY: `fcntl` は fd の旗を読み書きするだけである
    let ok = unsafe {
        let flags = libc::fcntl(fd, get);
        flags >= 0 && libc::fcntl(fd, set, flags | flag) == 0
    };
    if ok {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
impl Drop for Doorbell {
    fn drop(&mut self) {
        // SAFETY: 2つの fd はこの値だけが持ち、ここで1回だけ閉じる
        unsafe {
            libc::close(self.read);
            libc::close(self.write);
        }
    }
}

/// ハンドラが `errno` を変えたままにすると、割り込まれた処理が誤った値を読む。場所を返す関数の名前は OS ごとに違う。
#[cfg(unix)]
mod errno {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    use libc::__errno_location as location;
    #[cfg(any(target_vendor = "apple", target_os = "freebsd"))]
    use libc::__error as location;
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_vendor = "apple",
        target_os = "freebsd"
    )))]
    compile_error!("the signal doorbell does not know where this OS keeps `errno`");

    pub(super) fn get() -> libc::c_int {
        // SAFETY: `location` は呼んだスレッドの生きている `errno` を指す
        unsafe { *location() }
    }

    pub(super) fn set(value: libc::c_int) {
        // SAFETY: 同上
        unsafe { *location() = value }
    }
}

#[cfg(not(unix))]
impl Doorbell {
    pub fn new() -> io::Result<Arc<Doorbell>> {
        Ok(Arc::new(Doorbell {
            rung: std::sync::Mutex::new(false),
            bell: std::sync::Condvar::new(),
        }))
    }

    /// コンソールの handler は OS が作る普通のスレッドで動くので、ロックを取ってよい。
    pub fn ring(&self) {
        *self.lock() = true;
        self.bell.notify_one();
    }

    /// 鳴るか `timeout` が過ぎるまで待ち、旗を下ろす。何度鳴っても1回の起床にまとまる。鳴っていたかを返す。
    pub fn wait(&self, timeout: Option<Duration>) -> bool {
        let rung = self.lock();
        let mut rung = match timeout {
            None => self
                .bell
                .wait_while(rung, |rung| !*rung)
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            Some(timeout) => {
                self.bell
                    .wait_timeout_while(rung, timeout, |rung| !*rung)
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0
            }
        };
        std::mem::replace(&mut *rung, false)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, bool> {
        self.rung
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::Doorbell;

    /// 鳴らないはずの待ちの長さ。
    const SHORT: Duration = Duration::from_millis(20);

    /// 鳴るはずの待ちの上限。鳴らしが届かなければ、待ち続けずに落ちる。
    const LONG: Duration = Duration::from_secs(1);

    /// `wait` は早く戻ってよいので、`LONG` の間は待ち直す。
    fn rung_within(doorbell: &Doorbell) -> bool {
        let start = Instant::now();
        while start.elapsed() < LONG {
            if doorbell.wait(Some(LONG - start.elapsed())) {
                return true;
            }
        }
        false
    }

    #[test]
    fn a_ring_wakes_a_waiter() {
        let doorbell = Doorbell::new().unwrap();
        let ringer = Arc::clone(&doorbell);
        let thread = thread::spawn(move || {
            thread::sleep(SHORT);
            ringer.ring();
        });
        assert!(rung_within(&doorbell));
        thread.join().unwrap();
    }

    #[test]
    fn rings_merge() {
        let doorbell = Doorbell::new().unwrap();
        doorbell.ring();
        doorbell.ring();
        assert!(rung_within(&doorbell));
        assert!(!doorbell.wait(Some(SHORT)));
    }

    #[test]
    fn two_doorbells_do_not_share_rings() {
        let (one, two) = (Doorbell::new().unwrap(), Doorbell::new().unwrap());
        one.ring();
        assert!(!two.wait(Some(SHORT)));
        assert!(rung_within(&one));
    }
}
