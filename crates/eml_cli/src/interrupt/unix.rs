//! SIGINT のハンドラ。`SA_RESTART` を外すので、待っている読みは `EINTR` で戻り、`FileIn.read_all` が止める語を読んで
//! 止まる (docs/spec/object-model.md の「読み手」)。

use std::io;

use super::{INSTALLED, REPEAT_EXIT_CODE, Response};

pub(super) fn register() -> io::Result<()> {
    // `SA_RESTART` を入れないので、ハンドラの後で待っている読みが `EINTR` で戻る。`SA_NODEFER` を入れないので、
    // ハンドラが動く間は同じ SIGINT が塞がれ、`respond` が止める語を読んでから終わるかを決める間に入れ子にならない
    let handler: extern "C" fn(libc::c_int) = on_sigint;
    // `sigaction` は構造体と空の集合のほかに前提を持たない
    let result = unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handler as libc::sighandler_t;
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut())
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// async-signal-safe なことだけをする。アトミックな読み書きと `_exit` で、`errno` を変える関数は呼ばない。
extern "C" fn on_sigint(_: libc::c_int) {
    if let Some(Response::Exit) = INSTALLED.get().map(|installed| installed.respond()) {
        // まだ書いていない出力は捨てる (docs/spec/vm.md の「上限と割り込み」)
        unsafe { libc::_exit(REPEAT_EXIT_CODE.into()) }
    }
}
