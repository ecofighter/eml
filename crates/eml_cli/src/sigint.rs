//! SIGINT のハンドラ (docs/spec/vm.md の「上限と割り込み」)。unix だけで入れる。`SA_RESTART` を外すので、待っている
//! 読みは `EINTR` で戻り、`Fs.read_all` が止める語を読んで止まる (docs/spec/object-model.md の「`File`」)。

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use eml_cli::Control;

/// ハンドラが読む `Control`。ハンドラの中ではロックを取れないので static に置く。`OnceLock::get` は初期化の後は
/// アトミックな読みだけである。
static CONTROL: OnceLock<Arc<Control>> = OnceLock::new();
/// 止める語が立った後の SIGINT でプロセスを終わらせるか。
static EXIT_ON_REPEAT: AtomicBool = AtomicBool::new(false);

/// SIGINT を受けたときの動き方。
pub enum Mode {
    /// 2回目の SIGINT でその場で終わる。安全点に来ない処理から抜ける道である。
    Run,
    /// SIGINT で終わらない。抜けられない処理からは Ctrl-\ で抜ける (docs/spec/repl.md の「残る制限」)。
    Repl,
}

/// `Control` と動き方を static に置いてから、ハンドラを入れる。逆の順だと、間に来た SIGINT が `Control` を見つけられ
/// ずに捨てられる。
pub fn install(control: Arc<Control>, mode: Mode) {
    EXIT_ON_REPEAT.store(matches!(mode, Mode::Run), Ordering::Relaxed);
    assert!(
        CONTROL.set(control).is_ok(),
        "the SIGINT handler is installed once per process"
    );
    // 入れられなくてもプログラムは実行できる。その場合 Ctrl-C は既定の動作でプロセスを終わらせる
    if let Err(error) = sigaction() {
        eprintln!("warning: cannot install the SIGINT handler: {error}");
    }
}

fn sigaction() -> io::Result<()> {
    // `SA_RESTART` を入れないので、ハンドラの後で待っている読みが `EINTR` で戻る。`SA_NODEFER` を入れないので、
    // ハンドラが動く間は同じ SIGINT が塞がれ、`on_sigint` が止める語を読んでから終わるかを決める間に入れ子にならない
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
    let Some(control) = CONTROL.get() else {
        return;
    };
    if EXIT_ON_REPEAT.load(Ordering::Relaxed) && control.stop_bits() & Control::INTERRUPT != 0 {
        // まだ書いていない出力は捨てる (docs/spec/vm.md の「上限と割り込み」)
        unsafe { libc::_exit(130) }
    }
    control.interrupt();
}
