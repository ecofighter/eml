//! SIGINT と SIGTERM のハンドラ。`SA_RESTART` を外すので、待っている読みは `EINTR` で戻り、`FileIn.read_all` が止める語を
//! 読んで止まる (docs/spec/object-model.md の「読み手」)。

use std::io;
use std::sync::atomic::{AtomicU32, Ordering};

use eml_cli::Control;

use super::{INSTALLED, Mode, Response};

/// SIGINT はどちらの動き方でも入れ、SIGTERM は `eml run` でだけ入れる。REPL のプロンプトでは止める語を読む者が
/// いないので、SIGTERM は既定の動作でプロセスを終わらせる (docs/spec/vm.md の「上限と割り込み」)。
pub(super) fn register(mode: Mode) -> io::Result<()> {
    set(libc::SIGINT, on_sigint)?;
    if matches!(mode, Mode::Run) {
        set(libc::SIGTERM, on_sigterm)?;
    }
    Ok(())
}

/// `SA_RESTART`を入れないので、ハンドラの後で待っている読みが`EINTR`で戻る。`SA_NODEFER`を入れるので、ハンドラが
/// 動く間もシグナルは塞がれず、worker 0が受けるスレッドであり続ける。すべてのスレッドが塞いでいると、macOSは次の
/// シグナルをrunの終わりまでほかのスレッドの保留に残す (docs/spec/vm.md の「上限と割り込み」)。
fn set(signal: libc::c_int, handler: extern "C" fn(libc::c_int)) -> io::Result<()> {
    // `sigaction` は構造体と空の集合のほかに前提を持たない
    let result = unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handler as libc::sighandler_t;
        action.sa_flags = libc::SA_NODEFER;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(signal, &action, std::ptr::null_mut())
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// 種類ごとの、届いてまだ扱い終えていないシグナルの数。0でない数を見たハンドラはすぐに戻り、動いているハンドラが
/// 自分の分の後でそのシグナルを扱う。
static INTERRUPTS: AtomicU32 = AtomicU32::new(0);
static TERMINATES: AtomicU32 = AtomicU32::new(0);

extern "C" fn on_sigint(_: libc::c_int) {
    serialized(&INTERRUPTS, || respond(Control::INTERRUPT));
}

extern "C" fn on_sigterm(_: libc::c_int) {
    serialized(&TERMINATES, || respond(Control::TERMINATE));
}

/// このシグナルについて`handle`を1回動かし、その間に届いた同じ種類のシグナルについても1回ずつ、順に、入れ子に
/// せずに動かす。`SA_NODEFER`では2回目のシグナルがハンドラを割り込みうるが、`respond`は止める語を読んでから書くので、
/// 2つが重なってはならない。ハンドラに足すのはアトミックな操作だけである。
fn serialized(count: &AtomicU32, mut handle: impl FnMut()) {
    if count.fetch_add(1, Ordering::SeqCst) != 0 {
        return;
    }
    loop {
        handle();
        if count.fetch_sub(1, Ordering::SeqCst) == 1 {
            return;
        }
    }
}

/// async-signal-safe なことだけをする。アトミックな読み書きと、自己パイプへの `write` と、`_exit` である。`write` が
/// 変えた `errno` は `Doorbell::ring` が戻す。
fn respond(signal: u32) {
    if let Some(Response::Exit(code)) = INSTALLED.get().map(|installed| installed.respond(signal)) {
        // まだ書いていない出力は捨てる (docs/spec/vm.md の「上限と割り込み」)
        unsafe { libc::_exit(code.into()) }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// 1回目のシグナルのハンドラの中で2回目が届いた形を、`serialized`を入れ子に呼んで作る。
    fn enter(count: &AtomicU32, calls: &Cell<u32>, depth: &Cell<u32>, deepest: &Cell<u32>) {
        serialized(count, || {
            depth.set(depth.get() + 1);
            deepest.set(deepest.get().max(depth.get()));
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                enter(count, calls, depth, deepest);
            }
            depth.set(depth.get() - 1);
        });
    }

    /// ハンドラの中で届いたシグナルは、入れ子にならず、ハンドラの後で扱う。
    #[test]
    fn a_signal_inside_the_handler_is_handled_after_it() {
        let count = AtomicU32::new(0);
        let (calls, depth, deepest) = (Cell::new(0), Cell::new(0), Cell::new(0));
        enter(&count, &calls, &depth, &deepest);
        assert_eq!(calls.get(), 2);
        assert_eq!(deepest.get(), 1);
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }
}
