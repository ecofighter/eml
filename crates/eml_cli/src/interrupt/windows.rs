//! コンソールの handler。std の標準入力はコンソールの読みが Ctrl-C で中断されても読み直し、pipe とファイルの読みは
//! Ctrl-C で戻らないので、待っている読みは戻ってから止める語を見る (docs/spec/vm.md の「上限と割り込み」)。

use std::io;

use windows_sys::Win32::Foundation::{FALSE, TRUE};
use windows_sys::Win32::System::Console::{CTRL_C_EVENT, SetConsoleCtrlHandler};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
use windows_sys::core::BOOL;

use super::{EXIT_CODE, INSTALLED, Installed, Response};

pub(super) fn register() -> io::Result<()> {
    // 関数を足すだけで、ほかの前提を持たない
    if unsafe { SetConsoleCtrlHandler(Some(on_console_event), TRUE) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// OS が作ったスレッドで動き、プログラムのスレッドと並んで走る。static を読み、終わり方を渡すことだけをする。
unsafe extern "system" fn on_console_event(event: u32) -> BOOL {
    console_event(INSTALLED.get(), event, terminate)
}

/// Ctrl-C のほかは FALSE を返して OS の既定の handler に任せる。Ctrl-Break は REPL から抜ける道で、閉じることと
/// シャットダウンはプロセスを終わらせる (docs/spec/vm.md の「上限と割り込み」)。
fn console_event(installed: Option<&Installed>, event: u32, exit: impl FnOnce(u32)) -> BOOL {
    if event != CTRL_C_EVENT {
        return FALSE;
    }
    let Some(installed) = installed else {
        return FALSE;
    };
    if installed.respond() == Response::Exit {
        exit(EXIT_CODE.into());
    }
    TRUE
}

/// `ExitProcess` はほかのスレッドを止めた後に DLL の後始末を呼び、止めたスレッドが持っていたロックで待ちうる。
/// `_exit` と同じく、後始末をせずに終わらせる。まだ書いていない出力は捨てる。
fn terminate(code: u32) {
    // 自分のプロセスの擬似ハンドルを渡すだけで、ほかの前提を持たない
    unsafe { TerminateProcess(GetCurrentProcess(), code) };
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use eml_cli::Control;
    use windows_sys::Win32::System::Console::{
        CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
    };

    use super::super::Mode;
    use super::super::tests::installed;
    use super::*;

    #[test]
    fn the_console_handler_takes_ctrl_c_and_ends_a_run_on_the_second() {
        let installed = installed(Mode::Run);
        let exited = Cell::new(None);
        assert_eq!(
            console_event(Some(&installed), CTRL_C_EVENT, |code| exited
                .set(Some(code))),
            TRUE
        );
        assert_eq!(installed.control.stop_bits(), Control::INTERRUPT);
        assert_eq!(exited.get(), None);
        assert_eq!(
            console_event(Some(&installed), CTRL_C_EVENT, |code| exited
                .set(Some(code))),
            TRUE
        );
        assert_eq!(exited.get(), Some(130));
    }

    #[test]
    fn the_console_handler_leaves_the_other_events_to_the_system() {
        let installed = installed(Mode::Run);
        for event in [
            CTRL_BREAK_EVENT,
            CTRL_CLOSE_EVENT,
            CTRL_LOGOFF_EVENT,
            CTRL_SHUTDOWN_EVENT,
        ] {
            assert_eq!(
                console_event(Some(&installed), event, |_| panic!("exited")),
                FALSE
            );
        }
        assert_eq!(installed.control.stop_bits(), 0);
        assert_eq!(
            console_event(None, CTRL_C_EVENT, |_| panic!("exited")),
            FALSE
        );
    }
}
