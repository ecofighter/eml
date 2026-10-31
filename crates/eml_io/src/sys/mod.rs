//! OS ごとの backend (docs/spec/vm.md の「時計と driver」)。Linux は epoll、Apple は kqueue で、どちらも `unix.rs` の
//! `Unix` が fd の表と歩を持ち、OS ごとのファイルは登録と待ちと起こしだけを持つ。Windows は IOCP で、`iocp.rs` が
//! 表と歩と待ちを持つ。

use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

#[cfg(target_os = "linux")]
mod epoll;
#[cfg(windows)]
mod iocp;
#[cfg(target_vendor = "apple")]
mod kqueue;
#[cfg(unix)]
mod unix;

#[cfg(target_os = "linux")]
pub(crate) type Os = unix::Unix<epoll::Epoll>;
#[cfg(target_vendor = "apple")]
pub(crate) type Os = unix::Unix<kqueue::Kqueue>;
#[cfg(windows)]
pub(crate) type Os = iocp::Iocp;
#[cfg(not(any(target_os = "linux", target_vendor = "apple", windows)))]
compile_error!("eml_io has no I/O backend for this OS");

/// `d` をミリ秒へ切り上げ、`max` で切る。切り上げるので期限より早くは戻らず、`max` で切った待ちは早く戻るだけで
/// ある (docs/spec/vm.md の「時計と driver」)。期限のない待ちは、呼ぶ側が自分の OS の値に写し、ここへは渡さない。
// kqueue は timespec で待つので、Apple ではテストのほかに呼ぶ所がない
#[cfg_attr(all(target_vendor = "apple", not(test)), expect(dead_code))]
pub(crate) fn millis(d: Duration, max: u32) -> u32 {
    u32::try_from(d.as_nanos().div_ceil(1_000_000)).map_or(max, |ms| ms.min(max))
}

/// OS の解決器で `host` の `port` の番地を引く。解決器の誤りも空の一覧も `NotFound` にする
/// (docs/spec/stdlib.md の「`Std.Net`」)。driver の解決のスレッドの上で呼ばれ、ブロックしてよい。
pub(crate) fn lookup(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e.to_string()))?
        .collect();
    if addrs.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "no address"));
    }
    Ok(addrs)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::millis;

    #[test]
    fn millis_round_up_and_clamp_at_the_limit() {
        let max = 1000;
        assert_eq!(millis(Duration::ZERO, max), 0);
        assert_eq!(millis(Duration::from_nanos(1), max), 1);
        assert_eq!(millis(Duration::from_millis(1), max), 1);
        assert_eq!(
            millis(Duration::from_millis(1) + Duration::from_nanos(1), max),
            2
        );
        assert_eq!(millis(Duration::from_millis(1001), max), max);
        assert_eq!(
            millis(Duration::from_millis(u64::from(u32::MAX) + 1), u32::MAX),
            u32::MAX
        );
        assert_eq!(millis(Duration::MAX, max), max);
    }
}
