//! SIGINT と SIGTERM のハンドラ。`SA_RESTART` を外すので、待っている読みは `EINTR` で戻り、`FileIn.read_all` が止める語を
//! 読んで止まる (docs/spec/object-model.md の「読み手」)。

use std::io;

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

/// `SA_RESTART` を入れないので、ハンドラの後で待っている読みが `EINTR` で戻る。`SA_NODEFER` を入れないので、
/// ハンドラが動く間は同じシグナルが塞がれ、`respond` が止める語を読んでから終わるかを決める間に入れ子にならない。
fn set(signal: libc::c_int, handler: extern "C" fn(libc::c_int)) -> io::Result<()> {
    // `sigaction` は構造体と空の集合のほかに前提を持たない
    let result = unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handler as libc::sighandler_t;
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(signal, &action, std::ptr::null_mut())
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

extern "C" fn on_sigint(_: libc::c_int) {
    respond(Control::INTERRUPT);
}

extern "C" fn on_sigterm(_: libc::c_int) {
    respond(Control::TERMINATE);
}

/// async-signal-safe なことだけをする。アトミックな読み書きと `_exit` で、`errno` を変える関数は呼ばない。
fn respond(signal: u32) {
    if let Some(Response::Exit(code)) = INSTALLED.get().map(|installed| installed.respond(signal)) {
        // まだ書いていない出力は捨てる (docs/spec/vm.md の「上限と割り込み」)
        unsafe { libc::_exit(code.into()) }
    }
}
