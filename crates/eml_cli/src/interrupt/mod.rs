//! Ctrl-C の受け口 (docs/spec/vm.md の「上限と割り込み」)。unix では SIGINT のハンドラ、Windows ではコンソールの
//! handler が受け、どちらも static に置いた `Installed` の `respond` で何をするかを決める。

use std::sync::{Arc, OnceLock};

use eml_cli::Control;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix::register;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::register;

/// 受け口が読む `Control` と動き方。受け口の中ではロックを取れないので static に置く。`OnceLock::get` は初期化の後は
/// アトミックな読みだけである。受け口は読んだ値を引数で受ける関数に渡すので、テストは static を使わない。
static INSTALLED: OnceLock<Installed> = OnceLock::new();

struct Installed {
    control: Arc<Control>,
    mode: Mode,
}

/// Ctrl-C を受けたときの動き方。
#[derive(Clone, Copy)]
pub enum Mode {
    /// 2回目の Ctrl-C でその場で終わる。安全点に来ない処理から抜ける道である。
    Run,
    /// Ctrl-C で終わらない。抜けられない処理からは、unix では Ctrl-\、Windows では Ctrl-Break で抜ける
    /// (docs/spec/repl.md の「残る制限」)。
    Repl,
}

/// 2回目の Ctrl-C で終わるときの終了コード。1回目で止まったときと同じ値にする (docs/spec/vm.md の「上限と割り込み」)。
const REPEAT_EXIT_CODE: u8 = 130;

#[derive(Debug, PartialEq)]
enum Response {
    Interrupted,
    /// まだ書いていない出力を捨てて、その場で終わる。
    Exit,
}

/// `Control` と動き方を static に置いてから、受け口を入れる。逆の順だと、間に来た Ctrl-C が `Control` を見つけられ
/// ない。
pub fn install(control: Arc<Control>, mode: Mode) {
    assert!(
        INSTALLED.set(Installed { control, mode }).is_ok(),
        "the Ctrl-C handler is installed once per process"
    );
    // 入れられなくてもプログラムは実行できる。その場合 Ctrl-C は既定の動作でプロセスを終わらせる
    if let Err(error) = register() {
        eprintln!("warning: cannot install the Ctrl-C handler: {error}");
    }
}

impl Installed {
    /// アトミックな読み書きだけをするので、シグナルのハンドラからも呼べる。
    fn respond(&self) -> Response {
        if matches!(self.mode, Mode::Run) && self.control.stop_bits() & Control::INTERRUPT != 0 {
            return Response::Exit;
        }
        self.control.interrupt();
        Response::Interrupted
    }
}

#[cfg(test)]
mod tests {
    use eml_cli::Limits;

    use super::*;

    pub(super) fn installed(mode: Mode) -> Installed {
        Installed {
            control: Arc::new(Control::new(Limits::default())),
            mode,
        }
    }

    #[test]
    fn the_first_ctrl_c_sets_the_interrupt_bit() {
        for mode in [Mode::Run, Mode::Repl] {
            let installed = installed(mode);
            assert_eq!(installed.respond(), Response::Interrupted);
            assert_eq!(installed.control.stop_bits(), Control::INTERRUPT);
        }
    }

    #[test]
    fn a_second_ctrl_c_ends_a_run() {
        let installed = installed(Mode::Run);
        installed.respond();
        assert_eq!(installed.respond(), Response::Exit);
    }

    #[test]
    fn a_second_ctrl_c_only_interrupts_the_repl() {
        let installed = installed(Mode::Repl);
        installed.respond();
        assert_eq!(installed.respond(), Response::Interrupted);
        assert_eq!(installed.control.stop_bits(), Control::INTERRUPT);
    }

    #[test]
    fn a_ctrl_c_after_a_cleared_interrupt_is_a_first_one() {
        let installed = installed(Mode::Run);
        installed.respond();
        installed.control.clear();
        assert_eq!(installed.respond(), Response::Interrupted);
    }
}
