//! コンソールの handler。std の標準入力はコンソールの読みが Ctrl-C で中断されても読み直し、pipe とファイルの読みは
//! Ctrl-C で戻らないので、待っている読みは戻ってから止める語を見る (docs/spec/vm.md の「上限と割り込み」)。

use std::io;

use windows_sys::Win32::Foundation::{FALSE, TRUE};
use windows_sys::Win32::System::Console::{
    CTRL_C_EVENT, CTRL_CLOSE_EVENT, CTRL_SHUTDOWN_EVENT, SetConsoleCtrlHandler,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
use windows_sys::core::BOOL;

use eml_cli::Control;

use super::{INSTALLED, Installed, Mode, Response};

/// 動き方によらず同じ handler を入れ、閉じることとシャットダウンを受けるかは handler が動き方で決める。
pub(super) fn register(_: Mode) -> io::Result<()> {
    // 関数を足すだけで、ほかの前提を持たない
    if unsafe { SetConsoleCtrlHandler(Some(on_console_event), TRUE) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// handler が OS に返す答え。
#[derive(Debug, PartialEq)]
enum Action {
    /// FALSE を返し、OS の既定の handler に任せる。
    Pass,
    /// TRUE を返す。
    Handled,
    /// 返らない。閉じることとシャットダウンでは handler が返るとプロセスが終わるので、プログラムが自分で終わるか OS の
    /// 待ち時間が切れるまで、このスレッドを止めておく。
    Hold,
    /// この終了コードでその場で終わる。
    Exit(u8),
}

/// OS が作ったスレッドで動き、プログラムのスレッドと並んで走る。static を読み、終わり方を渡すことだけをする。
unsafe extern "system" fn on_console_event(event: u32) -> BOOL {
    act(console_event(INSTALLED.get(), event))
}

/// 決めた答えを行う。`Hold` では返らない。止めたスレッドはロックを持たないので、プロセスがどの道で終わっても妨げない。
fn act(action: Action) -> BOOL {
    match action {
        Action::Pass => FALSE,
        Action::Handled => TRUE,
        Action::Hold => loop {
            std::thread::park();
        },
        Action::Exit(code) => {
            terminate(code.into());
            TRUE
        }
    }
}

/// Ctrl-C は `Interrupt`、閉じることとシャットダウンは `eml run` でだけ `Terminate` にする。ほかは OS の既定の handler に
/// 任せる。閉じることとシャットダウンは2回目でも終わらせない。人が送り直すものでなく、安全点に来ない処理からは OS の
/// 待ち時間が出口になるためである (docs/spec/vm.md の「上限と割り込み」)。
fn console_event(installed: Option<&Installed>, event: u32) -> Action {
    let Some(installed) = installed else {
        return Action::Pass;
    };
    match event {
        CTRL_C_EVENT => match installed.respond(Control::INTERRUPT) {
            Response::Handled => Action::Handled,
            Response::Exit(code) => Action::Exit(code),
        },
        CTRL_CLOSE_EVENT | CTRL_SHUTDOWN_EVENT if matches!(installed.mode, Mode::Run) => {
            // 2回目の `Exit` は何も書き換えずに返るので、捨ててよい
            let _ = installed.respond(Control::TERMINATE);
            Action::Hold
        }
        _ => Action::Pass,
    }
}

/// `ExitProcess` はほかのスレッドを止めた後に DLL の後始末を呼び、止めたスレッドが持っていたロックで待ちうる。
/// `_exit` と同じく、後始末をせずに終わらせる。まだ書いていない出力は捨てる。
fn terminate(code: u32) {
    // 自分のプロセスの擬似ハンドルを渡すだけで、ほかの前提を持たない
    unsafe { TerminateProcess(GetCurrentProcess(), code) };
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_LOGOFF_EVENT};

    use super::super::tests::installed;
    use super::*;

    #[test]
    fn the_console_handler_takes_ctrl_c_and_ends_a_run_on_the_second() {
        let installed = installed(Mode::Run);
        assert_eq!(
            console_event(Some(&installed), CTRL_C_EVENT),
            Action::Handled
        );
        assert_eq!(installed.control.stop_bits(), Control::INTERRUPT);
        assert_eq!(
            console_event(Some(&installed), CTRL_C_EVENT),
            Action::Exit(130)
        );
    }

    #[test]
    fn a_watched_ctrl_c_is_handled_without_exit() {
        let installed = installed(Mode::Run);
        installed.control.set_watched(Control::INTERRUPT);
        assert_eq!(
            console_event(Some(&installed), CTRL_C_EVENT),
            Action::Handled
        );
        assert_eq!(installed.control.stop_bits(), 0);
        assert_eq!(installed.control.pending_signals(), Control::INTERRUPT);
    }

    #[test]
    fn close_and_shutdown_raise_terminate_and_hold_even_when_repeated() {
        for event in [CTRL_CLOSE_EVENT, CTRL_SHUTDOWN_EVENT] {
            let installed = installed(Mode::Run);
            assert_eq!(console_event(Some(&installed), event), Action::Hold);
            assert_eq!(installed.control.stop_bits(), Control::TERMINATE);
            assert_eq!(console_event(Some(&installed), event), Action::Hold);
            assert_eq!(installed.control.stop_bits(), Control::TERMINATE);
        }
    }

    #[test]
    fn a_watched_close_is_deferred_and_holds_the_handler() {
        let installed = installed(Mode::Run);
        installed.control.set_watched(Control::TERMINATE);
        assert_eq!(
            console_event(Some(&installed), CTRL_CLOSE_EVENT),
            Action::Hold
        );
        assert_eq!(installed.control.stop_bits(), 0);
        assert_eq!(installed.control.pending_signals(), Control::TERMINATE);
        assert_eq!(
            console_event(Some(&installed), CTRL_SHUTDOWN_EVENT),
            Action::Hold
        );
        assert_eq!(installed.control.pending_signals(), Control::TERMINATE);
    }

    #[test]
    fn the_console_handler_leaves_the_other_events_to_the_system() {
        let run = installed(Mode::Run);
        for event in [CTRL_BREAK_EVENT, CTRL_LOGOFF_EVENT] {
            assert_eq!(console_event(Some(&run), event), Action::Pass);
        }
        let repl = installed(Mode::Repl);
        for event in [
            CTRL_BREAK_EVENT,
            CTRL_CLOSE_EVENT,
            CTRL_LOGOFF_EVENT,
            CTRL_SHUTDOWN_EVENT,
        ] {
            assert_eq!(console_event(Some(&repl), event), Action::Pass);
        }
        assert_eq!(run.control.stop_bits(), 0);
        assert_eq!(repl.control.stop_bits(), 0);
        assert_eq!(console_event(None, CTRL_C_EVENT), Action::Pass);
        assert_eq!(console_event(None, CTRL_CLOSE_EVENT), Action::Pass);
    }

    /// 返らないことが、閉じた後もプロセスを生かしておく。返ればすぐ終わるので、待ちは短くてよい。
    #[test]
    fn hold_never_returns() {
        assert_eq!(act(Action::Pass), FALSE);
        assert_eq!(act(Action::Handled), TRUE);
        let held = std::thread::spawn(|| act(Action::Hold));
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!held.is_finished());
    }
}
