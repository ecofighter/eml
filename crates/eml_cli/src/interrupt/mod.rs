//! Ctrl-C と SIGTERM のハンドラ (docs/spec/vm.md の「上限と割り込み」)。unix では SIGINT と SIGTERM のハンドラ、
//! Windows ではコンソールの handler が受け、どれも static に置いた `Installed` の `respond` で何をするかを決める。

use std::sync::{Arc, OnceLock};

use eml_cli::{Control, Doorbell};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix::register;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::register;

/// ハンドラが読む `Control` と動き方。ハンドラの中ではロックを取れないので static に置く。`OnceLock::get` は初期化の後は
/// アトミックな読みだけである。ハンドラは読んだ値を引数で受ける関数に渡すので、テストは static を使わない。
static INSTALLED: OnceLock<Installed> = OnceLock::new();

struct Installed {
    control: Arc<Control>,
    /// 監視のスレッドを起こす自己パイプ。run の設定にも同じものを渡す。作れなかったときは `None` で、監視のスレッドは
    /// 刻みでシグナルを見に行く (docs/spec/vm.md の「タスクとスケジュール」)。
    doorbell: Option<Arc<Doorbell>>,
    mode: Mode,
}

/// シグナルを受けたときの動き方。
#[derive(Clone, Copy)]
pub enum Mode {
    /// 2回目の同じシグナルでその場で終わる。安全点に来ない処理から抜ける道である。SIGTERM も受ける。
    Run,
    /// Ctrl-C で終わらず、SIGTERM は受けない。抜けられない処理からは、unix では Ctrl-\、Windows では Ctrl-Break で抜ける
    /// (docs/spec/repl.md の「残る制限」)。
    Repl,
}

/// 割り込みで終わるときの終了コード。1回目で止まったときも、2回目の Ctrl-C で終わるときもこの値にする
/// (docs/spec/vm.md の「上限と割り込み」)。130 は、シェルが SIGINT で終わった子に報告する値である。
pub const INTERRUPT_EXIT_CODE: u8 = 130;

/// SIGTERM で終わるときの終了コード。130 と同じく、シェルが SIGTERM で終わった子に報告する値である。
pub const TERMINATE_EXIT_CODE: u8 = 143;

#[derive(Debug, PartialEq)]
enum Response {
    /// 止める語か保留のシグナルに書いて、プログラムに任せる。
    Handled,
    /// まだ書いていない出力を捨てて、この終了コードでその場で終わる。
    Exit(u8),
}

/// ハンドラと run の設定の両方に渡す自己パイプを作る。作れなくてもプログラムは実行でき、シグナルは監視の刻みで届く。
pub fn doorbell() -> Option<Arc<Doorbell>> {
    Doorbell::new()
        .inspect_err(|error| eprintln!("warning: cannot create the signal doorbell: {error}"))
        .ok()
}

/// `Control` と動き方を static に置いてから、ハンドラを入れる。逆の順だと、間に来た Ctrl-C が `Control` を見つけられ
/// ない。
pub fn install(control: Arc<Control>, doorbell: Option<Arc<Doorbell>>, mode: Mode) {
    assert!(
        INSTALLED
            .set(Installed {
                control,
                doorbell,
                mode
            })
            .is_ok(),
        "the signal handlers are installed once per process"
    );
    // 入れられなくてもプログラムは実行できる。その場合シグナルは既定の動作でプロセスを終わらせる
    if let Err(error) = register(mode) {
        eprintln!("warning: cannot install the signal handlers: {error}");
    }
}

impl Installed {
    /// シグナル `signal` (`Control::INTERRUPT` か `Control::TERMINATE`) を受けたときの動きを決める。アトミックな読み書き
    /// だけをするので、シグナルのハンドラからも呼べる。前の同じシグナルを届けていない間か、それがもう run を止めている間は、
    /// `eml run` ではその場で終わる。待つタスクがいれば届けるまで保留にし、いなければ run を止める (docs/spec/vm.md の
    /// 「上限と割り込み」)。2つの種類は互いのビットを見ない。終わらないときは自己パイプを鳴らし、監視のスレッドに保留を
    /// 中継させ、眠っている worker を起こさせる。
    fn respond(&self, signal: u32) -> Response {
        let control = &self.control;
        if matches!(self.mode, Mode::Run)
            && (control.stop_bits() | control.pending_signals()) & signal != 0
        {
            return Response::Exit(if signal == Control::INTERRUPT {
                INTERRUPT_EXIT_CODE
            } else {
                TERMINATE_EXIT_CODE
            });
        }
        if control.watched() & signal != 0 {
            control.defer(signal);
        } else if signal == Control::INTERRUPT {
            control.interrupt();
        } else {
            control.terminate();
        }
        if let Some(doorbell) = &self.doorbell {
            doorbell.ring();
        }
        Response::Handled
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use eml_cli::Limits;

    use super::*;

    const INTERRUPT: u32 = Control::INTERRUPT;
    const TERMINATE: u32 = Control::TERMINATE;

    pub(super) fn installed(mode: Mode) -> Installed {
        Installed {
            control: Arc::new(Control::new(Limits::default())),
            doorbell: None,
            mode,
        }
    }

    /// 終わらないときはどちらの手順でも自己パイプを鳴らし、その場で終わるときは鳴らさない。
    #[test]
    fn a_handled_signal_rings_the_doorbell() {
        let doorbell = Doorbell::new().unwrap();
        let installed = Installed {
            doorbell: Some(Arc::clone(&doorbell)),
            ..installed(Mode::Run)
        };
        installed.control.set_watched(TERMINATE);
        assert_eq!(installed.respond(TERMINATE), Response::Handled);
        assert!(doorbell.wait(Some(Duration::ZERO)));
        assert_eq!(installed.respond(INTERRUPT), Response::Handled);
        assert!(doorbell.wait(Some(Duration::ZERO)));
        assert_eq!(installed.respond(INTERRUPT), Response::Exit(130));
        assert!(!doorbell.wait(Some(Duration::ZERO)));
    }

    #[test]
    fn the_first_ctrl_c_sets_the_interrupt_bit() {
        for mode in [Mode::Run, Mode::Repl] {
            let installed = installed(mode);
            assert_eq!(installed.respond(INTERRUPT), Response::Handled);
            assert_eq!(installed.control.stop_bits(), Control::INTERRUPT);
        }
    }

    #[test]
    fn a_second_ctrl_c_ends_a_run() {
        let installed = installed(Mode::Run);
        installed.respond(INTERRUPT);
        assert_eq!(installed.respond(INTERRUPT), Response::Exit(130));
    }

    #[test]
    fn a_second_ctrl_c_only_interrupts_the_repl() {
        let installed = installed(Mode::Repl);
        installed.respond(INTERRUPT);
        assert_eq!(installed.respond(INTERRUPT), Response::Handled);
        assert_eq!(installed.control.stop_bits(), Control::INTERRUPT);
    }

    #[test]
    fn a_ctrl_c_after_a_cleared_interrupt_is_a_first_one() {
        let installed = installed(Mode::Run);
        installed.respond(INTERRUPT);
        installed.control.clear();
        assert_eq!(installed.respond(INTERRUPT), Response::Handled);
    }

    #[test]
    fn a_watched_ctrl_c_is_deferred() {
        let installed = installed(Mode::Run);
        installed.control.set_watched(INTERRUPT);
        assert_eq!(installed.respond(INTERRUPT), Response::Handled);
        assert_eq!(installed.control.stop_bits(), 0);
        assert_eq!(installed.control.pending_signals(), INTERRUPT);
    }

    #[test]
    fn a_ctrl_c_while_one_is_pending_ends_a_run() {
        let installed = installed(Mode::Run);
        installed.control.set_watched(INTERRUPT);
        installed.respond(INTERRUPT);
        assert_eq!(installed.respond(INTERRUPT), Response::Exit(130));
    }

    #[test]
    fn a_ctrl_c_after_a_settled_one_is_a_first_one() {
        let installed = installed(Mode::Run);
        installed.control.set_watched(INTERRUPT);
        installed.respond(INTERRUPT);
        installed.control.settle(INTERRUPT);
        assert_eq!(installed.respond(INTERRUPT), Response::Handled);
        assert_eq!(installed.control.pending_signals(), INTERRUPT);
        // 待つタスクがもういなければ、届けた後の1回目は run を止める
        installed.control.settle(INTERRUPT);
        installed.control.set_watched(0);
        assert_eq!(installed.respond(INTERRUPT), Response::Handled);
        assert_eq!(installed.control.stop_bits(), INTERRUPT);
    }

    #[test]
    fn a_watched_terminate_is_deferred() {
        let installed = installed(Mode::Run);
        installed.control.set_watched(TERMINATE);
        assert_eq!(installed.respond(TERMINATE), Response::Handled);
        assert_eq!(installed.control.stop_bits(), 0);
        assert_eq!(installed.control.pending_signals(), TERMINATE);
    }

    #[test]
    fn an_unwatched_terminate_raises_terminate() {
        let installed = installed(Mode::Run);
        // 待つタスクがいるのが別の種類なら、保留にしない
        installed.control.set_watched(INTERRUPT);
        assert_eq!(installed.respond(TERMINATE), Response::Handled);
        assert_eq!(installed.control.stop_bits(), TERMINATE);
        assert_eq!(installed.control.worker_bits(), TERMINATE);
        assert_eq!(installed.control.pending_signals(), 0);
    }

    #[test]
    fn a_terminate_while_one_is_pending_ends_a_run_with_143() {
        let installed = installed(Mode::Run);
        installed.control.set_watched(TERMINATE);
        installed.respond(TERMINATE);
        assert_eq!(installed.respond(TERMINATE), Response::Exit(143));
    }

    #[test]
    fn a_terminate_after_a_raised_one_ends_a_run_with_143() {
        let installed = installed(Mode::Run);
        installed.respond(TERMINATE);
        assert_eq!(installed.respond(TERMINATE), Response::Exit(143));
    }

    #[test]
    fn the_two_kinds_do_not_arm_each_other() {
        let installed = installed(Mode::Run);
        installed.respond(TERMINATE);
        assert_eq!(installed.respond(INTERRUPT), Response::Handled);
        assert_eq!(installed.control.stop_bits(), TERMINATE | INTERRUPT);

        let installed = self::installed(Mode::Run);
        installed.control.set_watched(INTERRUPT | TERMINATE);
        installed.respond(INTERRUPT);
        assert_eq!(installed.respond(TERMINATE), Response::Handled);
        assert_eq!(installed.control.pending_signals(), INTERRUPT | TERMINATE);
        assert_eq!(installed.control.stop_bits(), 0);
    }

    #[test]
    fn the_repl_defers_a_watched_ctrl_c_and_never_exits() {
        let installed = installed(Mode::Repl);
        installed.control.set_watched(INTERRUPT);
        for _ in 0..2 {
            assert_eq!(installed.respond(INTERRUPT), Response::Handled);
        }
        assert_eq!(installed.control.stop_bits(), 0);
        assert_eq!(installed.control.pending_signals(), INTERRUPT);
    }
}
