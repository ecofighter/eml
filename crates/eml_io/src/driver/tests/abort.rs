//! `Drop` がプロセスを止める道のテスト。テストのバイナリを子として起こし、子の中で driver を落とす。Miri はプロセスを
//! 起こせないので、Miri では流さない (docs/implementation/testing.md の「核の Miri」)。

use std::io;
use std::process::Command;

use eml_rt::{IoDriver, Op, RawBuf};

use super::super::HELD_ABORT;
use super::super::backend::{Cancel, Hold};
use super::fake::Reply;
use super::{buf, driver, listen, pending};

/// これがあればテストは子として driver を落とす。
const CHILD: &str = "EML_IO_ABORT_CHILD";

/// 子の中では `body` を流す。親では、同じテストだけを流す子を起こし、子が文言を書いて止まったことを確かめる。
fn in_child(path: &str, body: impl FnOnce()) {
    if std::env::var_os(CHILD).is_some() {
        body();
        return;
    }
    // libtest のテストの道は crate の名前を含まない
    let name = path.strip_prefix("eml_io::").expect("a path in eml_io");
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "the child did not abort: {stderr}");
    assert!(stderr.contains(HELD_ABORT), "{stderr}");
}

/// カーネルが buffer を持つ読みを1つ出した driver。
fn holding(data: &mut [u8]) -> super::super::Driver<super::fake::Fake> {
    let mut driver = driver();
    let h = listen(&mut driver);
    driver.backend.script(1, [Reply::Pending(Hold::Buffer)]);
    let buf: RawBuf = buf(data);
    pending(driver.submit(1, Op::Read { handle: h, buf }));
    driver
}

#[test]
#[cfg_attr(miri, ignore = "Miri はプロセスを起こせない")]
fn a_broken_driver_with_a_held_buffer_aborts_on_drop() {
    in_child(
        concat!(
            module_path!(),
            "::a_broken_driver_with_a_held_buffer_aborts_on_drop"
        ),
        || {
            let mut data = [0u8; 4];
            let mut driver = holding(&mut data);
            driver.backend.wait_error = Some(io::Error::other("the poller failed"));
            assert!(driver.wait(None).is_err());
            drop(driver);
        },
    );
}

#[test]
#[cfg_attr(miri, ignore = "Miri はプロセスを起こせない")]
fn a_held_buffer_past_the_limit_aborts_on_drop() {
    in_child(
        concat!(
            module_path!(),
            "::a_held_buffer_past_the_limit_aborts_on_drop"
        ),
        || {
            let mut data = [0u8; 4];
            let mut driver = holding(&mut data);
            // 取り消しの終わりが届かないので、偽の時計が `HELD_LIMIT` を越える
            driver.backend.cancels.push_back(Ok(Cancel::Pending));
            drop(driver);
        },
    );
}
